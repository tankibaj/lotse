//! Today's tokens and cost (status bar): summed from the Claude Code transcripts the daemon knows.
//!
//! The daemon learns a transcript's path from a hook (`transcript_path`), validated by
//! [`crate::transcript::validate_path`]. [`UsageTracker::refresh`] then reads what changed:
//!
//! - Per file it remembers the modification time, the length and the byte offset up to the last
//!   complete line, so a refresh only reads what was appended. A file that did not change costs one
//!   `stat`; a file last written before today's local midnight is not read at all.
//! - Claude Code writes one line per content block of an assistant message, each repeating the
//!   message's `usage`. Entries are keyed by message id (and request id): the latest line of a
//!   message wins, so a message counts once.
//! - Only assistant messages timestamped within today (local time) count.
//! - Cost uses [`price_for`], a small built-in table (no network). A message of a model without a
//!   price makes the whole cost unknown (`cost_micros: None`, "tokens only").
//!
//! **Unverified:** the prices were written from memory of Anthropic's public price list, not read
//! from it (no web access); check [`PRICES`] before relying on the dollar figure. Models newer than
//! the table are deliberately absent: they show tokens only until someone adds them.

use crate::transcript::parse_iso_utc_ms;
use lotse_proto::{UnixMs, UsageToday};
use serde_json::Value;
use std::collections::HashMap;
use std::io::{BufRead, BufReader, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

/// A transcript path is written to the database again after this long (keeps its retention fresh).
const PERSIST_EVERY: Duration = Duration::from_secs(10 * 60);

/// Local midnight to local midnight, in Unix milliseconds. `[start_ms, end_ms)`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DayBounds {
    pub start_ms: UnixMs,
    pub end_ms: UnixMs,
}

impl DayBounds {
    /// The local day that contains `now_ms` (UTC day if the time zone cannot be read).
    #[allow(unsafe_code)]
    pub fn local(now_ms: UnixMs) -> Self {
        use nix::libc;
        let utc = || {
            let start_ms = now_ms - now_ms % 86_400_000;
            DayBounds {
                start_ms,
                end_ms: start_ms + 86_400_000,
            }
        };
        let secs = (now_ms / 1000) as libc::time_t;
        // SAFETY: `tm` is plain data owned here; `localtime_r` and `mktime` only read and write it
        // (and the C library's time-zone state, which they initialise themselves).
        let (start, end) = unsafe {
            let mut tm: libc::tm = std::mem::zeroed();
            if libc::localtime_r(&secs, &mut tm).is_null() {
                return utc();
            }
            tm.tm_hour = 0;
            tm.tm_min = 0;
            tm.tm_sec = 0;
            tm.tm_isdst = -1;
            let start = libc::mktime(&mut tm);
            tm.tm_mday += 1;
            tm.tm_isdst = -1;
            (start, libc::mktime(&mut tm))
        };
        match (u64::try_from(start), u64::try_from(end)) {
            (Ok(s), Ok(e)) if s <= now_ms / 1000 && e > now_ms / 1000 => DayBounds {
                start_ms: s * 1000,
                end_ms: e * 1000,
            },
            _ => utc(),
        }
    }

    fn contains(&self, ms: UnixMs) -> bool {
        (self.start_ms..self.end_ms).contains(&ms)
    }
}

/// US dollars per million tokens, in millionths of a dollar (`$3` is `3_000_000`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Price {
    pub input: u64,
    pub output: u64,
}

impl Price {
    /// Prompt-cache reads cost a tenth of the input price.
    fn cache_read(self) -> u64 {
        self.input / 10
    }

    /// Cache writes (5-minute) cost 1.25 times the input price. Claude Code also writes 1-hour
    /// entries at twice the price; they are counted at 1.25 times here (a small underestimate).
    fn cache_write(self) -> u64 {
        self.input / 4 * 5
    }
}

const fn usd(dollars_x100: u64) -> u64 {
    dollars_x100 * 10_000
}

/// Model id prefix → price. First match wins. Older and current Claude families only.
pub const PRICES: &[(&str, Price)] = &[
    (
        "claude-opus-4-5",
        Price {
            input: usd(500),
            output: usd(2500),
        },
    ),
    (
        "claude-opus-4-1",
        Price {
            input: usd(1500),
            output: usd(7500),
        },
    ),
    (
        "claude-opus-4-2",
        Price {
            input: usd(1500),
            output: usd(7500),
        },
    ), // claude-opus-4-20250514
    (
        "claude-opus-4-0",
        Price {
            input: usd(1500),
            output: usd(7500),
        },
    ),
    (
        "claude-3-opus",
        Price {
            input: usd(1500),
            output: usd(7500),
        },
    ),
    (
        "claude-sonnet-4",
        Price {
            input: usd(300),
            output: usd(1500),
        },
    ),
    (
        "claude-3-7-sonnet",
        Price {
            input: usd(300),
            output: usd(1500),
        },
    ),
    (
        "claude-3-5-sonnet",
        Price {
            input: usd(300),
            output: usd(1500),
        },
    ),
    (
        "claude-haiku-4-5",
        Price {
            input: usd(100),
            output: usd(500),
        },
    ),
    (
        "claude-3-5-haiku",
        Price {
            input: usd(80),
            output: usd(400),
        },
    ),
    (
        "claude-3-haiku",
        Price {
            input: usd(25),
            output: usd(125),
        },
    ),
];

/// The price of `model`, `None` for a model the table does not know.
pub fn price_for(model: &str) -> Option<Price> {
    PRICES
        .iter()
        .find(|(prefix, _)| model.starts_with(prefix))
        .map(|(_, p)| *p)
}

#[derive(Debug, Clone, Default)]
struct Entry {
    model: String,
    input: u64,
    output: u64,
    cache_read: u64,
    cache_write: u64,
}

/// What one file adds to today's total.
#[derive(Debug, Clone, Default)]
struct Sum {
    input: u64,
    output: u64,
    cache_read: u64,
    cache_write: u64,
    /// Cost in millionths of a dollar times a million (divide once, at the end: no rounding drift).
    cost: u128,
    /// A message of a model without a price took part.
    unpriced: bool,
}

#[derive(Debug, Default)]
struct FileState {
    mtime: Option<SystemTime>,
    len: u64,
    /// Bytes consumed: up to and including the last complete line read.
    offset: u64,
    entries: HashMap<String, Entry>,
    sum: Sum,
}

impl FileState {
    fn resum(&mut self) {
        let mut sum = Sum::default();
        for e in self.entries.values() {
            sum.input += e.input;
            sum.output += e.output;
            sum.cache_read += e.cache_read;
            sum.cache_write += e.cache_write;
            match price_for(&e.model) {
                Some(p) => {
                    sum.cost += u128::from(e.input) * u128::from(p.input)
                        + u128::from(e.output) * u128::from(p.output)
                        + u128::from(e.cache_read) * u128::from(p.cache_read())
                        + u128::from(e.cache_write) * u128::from(p.cache_write());
                }
                None => sum.unpriced = true,
            }
        }
        self.sum = sum;
    }
}

struct Inner {
    day: Option<DayBounds>,
    files: HashMap<PathBuf, FileState>,
    persisted: HashMap<PathBuf, Instant>,
    last: UsageToday,
    /// Cost of the priced messages only (`last.cost_micros` is `None` when any is unpriced).
    priced_micros: u64,
}

/// See the module docs. Cheap to share: every method takes `&self`.
pub struct UsageTracker {
    inner: Mutex<Inner>,
    /// Held for a whole [`UsageTracker::refresh`]; `inner` only for its short lock-held steps.
    refreshing: Mutex<()>,
    persist_every: Duration,
}

impl Default for UsageTracker {
    fn default() -> Self {
        Self::new()
    }
}

impl UsageTracker {
    pub fn new() -> Self {
        Self::with_persist_every(PERSIST_EVERY)
    }

    /// [`Self::add`] asks for a database write of a known path after `persist_every`.
    pub fn with_persist_every(persist_every: Duration) -> Self {
        Self {
            persist_every,
            inner: Mutex::new(Inner {
                day: None,
                files: HashMap::new(),
                persisted: HashMap::new(),
                // nothing followed yet reads as zero tokens, $0.00 (an empty sum), not as "unknown"
                last: UsageToday {
                    cost_micros: Some(0),
                    ..UsageToday::default()
                },
                priced_micros: 0,
            }),
            refreshing: Mutex::new(()),
        }
    }

    fn lock(&self) -> MutexGuard<'_, Inner> {
        self.inner.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Start following a (validated) transcript. `true` when the caller should write the path to the
    /// database: it is new, or was last written a while ago.
    pub fn add(&self, path: PathBuf) -> bool {
        let mut g = self.lock();
        g.files.entry(path.clone()).or_default();
        let now = Instant::now();
        match g.persisted.get(&path) {
            Some(at) if now.duration_since(*at) < self.persist_every => false,
            _ => {
                g.persisted.insert(path, now);
                true
            }
        }
    }

    /// The total of the last [`Self::refresh`] (no file access).
    pub fn current(&self) -> UsageToday {
        self.lock().last.clone()
    }

    /// The cost of the last [`Self::refresh`] over the messages that have a price, in millionths of
    /// a dollar: [`UsageToday::cost_micros`] without the all-or-nothing `None` (the per-session
    /// panel shows a partial figure next to `priced: false`).
    pub fn priced_micros(&self) -> u64 {
        self.lock().priced_micros
    }

    /// Read what changed in the followed transcripts and recompute today's total. `Some(total)` when
    /// it differs from the previous one.
    pub fn refresh(&self, day: DayBounds) -> Option<UsageToday> {
        self.refresh_with(day, scan_file)
    }

    /// [`Self::refresh`] with the per-file `stat` and read swapped out. They run without the
    /// tracker lock, so [`Self::current`] (the snapshot) and [`Self::add`] (the hook path) never
    /// wait on file I/O: the file states are taken out, scanned, and put back.
    fn refresh_with(
        &self,
        day: DayBounds,
        scan: impl Fn(&Path, &mut FileState, DayBounds),
    ) -> Option<UsageToday> {
        // one refresh at a time: a second would find the taken-out states empty and re-read them
        let _one = self.refreshing.lock().unwrap_or_else(|e| e.into_inner());
        let mut work: Vec<(PathBuf, FileState)> = {
            let mut g = self.lock();
            if g.day != Some(day) {
                // a new day: everything counted so far belonged to yesterday
                g.day = Some(day);
                for f in g.files.values_mut() {
                    *f = FileState::default();
                }
            }
            g.files
                .iter_mut()
                .map(|(path, st)| (path.clone(), std::mem::take(st)))
                .collect()
        };
        for (path, st) in &mut work {
            scan(path, st, day);
        }
        let mut g = self.lock();
        for (path, st) in work {
            g.files.insert(path, st);
        }
        let mut total = Sum::default();
        for st in g.files.values() {
            total.input += st.sum.input;
            total.output += st.sum.output;
            total.cache_read += st.sum.cache_read;
            total.cache_write += st.sum.cache_write;
            total.cost += st.sum.cost;
            total.unpriced |= st.sum.unpriced;
        }
        let usage = UsageToday {
            input_tokens: total.input,
            output_tokens: total.output,
            cache_read_tokens: total.cache_read,
            cache_write_tokens: total.cache_write,
            cost_micros: (!total.unpriced).then_some((total.cost / 1_000_000) as u64),
        };
        g.priced_micros = (total.cost / 1_000_000) as u64;
        if usage == g.last {
            return None;
        }
        g.last = usage.clone();
        Some(usage)
    }
}

/// Bring one followed file up to date: a `stat`, and a read of what was appended if it changed.
fn scan_file(path: &Path, st: &mut FileState, day: DayBounds) {
    let Ok(meta) = std::fs::metadata(path) else {
        return;
    };
    let (len, mtime) = (meta.len(), meta.modified().ok());
    let day_start = UNIX_EPOCH + Duration::from_millis(day.start_ms);
    if mtime.is_some_and(|m| m < day_start) {
        return; // last written before today: nothing of today in it
    }
    if st.mtime == mtime && st.len == len {
        return;
    }
    if len < st.offset {
        *st = FileState::default(); // shorter than what was read: rewritten
    }
    if read_new_lines(path, st, day).is_ok() {
        st.mtime = mtime;
        st.len = len;
        st.resum();
    }
}

/// Read the complete lines after `st.offset`, adding today's usage entries.
fn read_new_lines(path: &Path, st: &mut FileState, day: DayBounds) -> std::io::Result<()> {
    let mut file = crate::transcript::open_nofollow(path)?;
    file.seek(SeekFrom::Start(st.offset))?;
    let mut reader = BufReader::new(file);
    let mut line = Vec::new();
    loop {
        line.clear();
        let n = reader.read_until(b'\n', &mut line)?;
        if n == 0 || line.last() != Some(&b'\n') {
            break; // end of file, or a line still being written
        }
        st.offset += n as u64;
        if let Some((key, entry)) = parse_usage_line(&line, day) {
            st.entries.insert(key, entry);
        }
    }
    Ok(())
}

/// `(message key, usage)` of an assistant line of today that carries token counts.
fn parse_usage_line(line: &[u8], day: DayBounds) -> Option<(String, Entry)> {
    // most lines (user text, tool results) have no usage: skip them without parsing
    if !line.windows(7).any(|w| w == b"\"usage\"") {
        return None;
    }
    let v: Value = serde_json::from_slice(line).ok()?;
    if v.get("type")?.as_str()? != "assistant" {
        return None;
    }
    let at = parse_iso_utc_ms(v.get("timestamp")?.as_str()?)?;
    if !day.contains(at) {
        return None;
    }
    let msg = v.get("message")?;
    let usage = msg.get("usage")?;
    let n = |k: &str| usage.get(k).and_then(Value::as_u64).unwrap_or(0);
    let entry = Entry {
        model: msg
            .get("model")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_owned(),
        input: n("input_tokens"),
        output: n("output_tokens"),
        cache_read: n("cache_read_input_tokens"),
        cache_write: n("cache_creation_input_tokens"),
    };
    if entry.input + entry.output + entry.cache_read + entry.cache_write == 0 {
        return None; // e.g. the `<synthetic>` placeholder messages
    }
    let key = match (
        msg.get("id").and_then(Value::as_str),
        v.get("requestId").and_then(Value::as_str),
    ) {
        (Some(id), Some(req)) => format!("{id}:{req}"),
        (Some(id), None) => id.to_owned(),
        _ => v.get("uuid")?.as_str()?.to_owned(),
    };
    Some((key, entry))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    // 2026-11-02 00:00:00Z .. 2026-11-03 00:00:00Z: a fixed "today", independent of the time zone
    const DAY: DayBounds = DayBounds {
        start_ms: 1_793_577_600_000,
        end_ms: 1_793_664_000_000,
    };

    fn line(id: &str, model: &str, ts: &str, usage: (u64, u64, u64, u64)) -> String {
        format!(
            "{{\"type\":\"assistant\",\"timestamp\":\"{ts}\",\"requestId\":\"r-{id}\",\"message\":{{\"id\":\"{id}\",\"model\":\"{model}\",\"role\":\"assistant\",\"content\":[],\"usage\":{{\"input_tokens\":{},\"output_tokens\":{},\"cache_read_input_tokens\":{},\"cache_creation_input_tokens\":{}}}}}}}\n",
            usage.0, usage.1, usage.2, usage.3
        )
    }

    /// The file was last written during `DAY`, whatever the real clock says.
    fn stamp(path: &Path) {
        let at = UNIX_EPOCH + Duration::from_millis(DAY.start_ms + 10 * 3_600_000);
        let f = std::fs::OpenOptions::new().write(true).open(path).unwrap();
        f.set_modified(at).unwrap();
    }

    fn write_file(path: &Path, text: &str) {
        std::fs::write(path, text).unwrap();
        stamp(path);
    }

    /// The file's path is resolved, as `validate_path` hands it over (files are opened without
    /// following links, and the temp folder sits behind `/var`).
    fn tracker_with(text: &str) -> (UsageTracker, tempfile::TempDir, PathBuf) {
        let dir = tempfile::tempdir().unwrap();
        let path = std::fs::canonicalize(dir.path()).unwrap().join("s.jsonl");
        write_file(&path, text);
        let t = UsageTracker::new();
        t.add(path.clone());
        (t, dir, path)
    }

    fn append(path: &Path, text: &str) {
        let mut f = std::fs::OpenOptions::new().append(true).open(path).unwrap();
        f.write_all(text.as_bytes()).unwrap();
        drop(f);
        stamp(path);
    }

    const NOW: &str = "2026-11-02T10:00:00.000Z";

    #[test]
    fn a_file_swapped_for_a_link_after_validation_is_not_read() {
        let tmp = tempfile::tempdir().unwrap();
        // resolved, as `validate_path` hands the path over (the temp folder sits behind `/var`)
        let dir = std::fs::canonicalize(tmp.path()).unwrap();
        let (real, other) = (dir.join("s.jsonl"), dir.join("other.jsonl"));
        let text = line("m1", "claude-opus-4-5-20251101", NOW, (1_000, 0, 0, 0));
        write_file(&real, &text);
        let t = UsageTracker::new();
        t.add(real.clone());
        t.refresh(DAY);
        assert_eq!(t.current().input_tokens, 1_000, "read while it is a file");

        write_file(&other, &text);
        std::fs::remove_file(&real).unwrap();
        std::os::unix::fs::symlink(&other, &real).unwrap();
        let t = UsageTracker::new();
        t.add(real);
        t.refresh(DAY);
        assert_eq!(t.current().input_tokens, 0, "the link was followed");
    }

    #[test]
    fn sums_the_four_token_kinds_and_prices_them() {
        // Opus 4.5: $5 in, $25 out, cache read $0.50, cache write $6.25 per million
        let text = line(
            "m1",
            "claude-opus-4-5-20251101",
            NOW,
            (1_000_000, 100_000, 2_000_000, 400_000),
        );
        let (t, _d, _p) = tracker_with(&text);
        let u = t.refresh(DAY).unwrap();
        assert_eq!(
            (
                u.input_tokens,
                u.output_tokens,
                u.cache_read_tokens,
                u.cache_write_tokens
            ),
            (1_000_000, 100_000, 2_000_000, 400_000)
        );
        assert_eq!(u.total_tokens(), 3_500_000);
        // 5.00 + 2.50 + 1.00 + 2.50
        assert_eq!(u.cost_micros, Some(11_000_000));
        assert_eq!(t.current(), u);
    }

    #[test]
    fn only_messages_dated_today_count() {
        let mut text = line(
            "old",
            "claude-sonnet-4-5",
            "2026-11-01T23:59:59.999Z",
            (1000, 0, 0, 0),
        );
        text += &line(
            "new",
            "claude-sonnet-4-5",
            "2026-11-02T00:00:00.000Z",
            (7, 0, 0, 0),
        );
        text += &line(
            "next",
            "claude-sonnet-4-5",
            "2026-11-03T00:00:00.000Z",
            (900, 0, 0, 0),
        );
        let (t, _d, _p) = tracker_with(&text);
        assert_eq!(t.refresh(DAY).unwrap().input_tokens, 7);
    }

    #[test]
    fn a_message_written_as_several_lines_counts_once_with_its_latest_usage() {
        let mut text = line("m1", "claude-sonnet-4-5", NOW, (100, 1, 0, 0));
        text += &line("m1", "claude-sonnet-4-5", NOW, (100, 50, 0, 0)); // same message, later block
        text += &line("m2", "claude-sonnet-4-5", NOW, (10, 5, 0, 0));
        let (t, _d, _p) = tracker_with(&text);
        let u = t.refresh(DAY).unwrap();
        assert_eq!((u.input_tokens, u.output_tokens), (110, 55));
    }

    #[test]
    fn an_unknown_model_gives_tokens_only() {
        let mut text = line("a", "claude-sonnet-4-5", NOW, (1_000_000, 0, 0, 0));
        text += &line("b", "claude-fable-9-9", NOW, (500, 0, 0, 0));
        // a placeholder without tokens does not spoil the cost
        text += &line("c", "<synthetic>", NOW, (0, 0, 0, 0));
        let (t, _d, _p) = tracker_with(&text);
        let u = t.refresh(DAY).unwrap();
        assert_eq!(u.input_tokens, 1_000_500);
        assert_eq!(u.cost_micros, None);

        let (t, _d, _p) = tracker_with(&line("a", "claude-sonnet-4-5", NOW, (1_000_000, 0, 0, 0)));
        assert_eq!(t.refresh(DAY).unwrap().cost_micros, Some(3_000_000));
    }

    #[test]
    fn the_price_table_knows_families_and_refuses_guesses() {
        let p = |m: &str| price_for(m).map(|p| (p.input, p.output));
        assert_eq!(p("claude-opus-4-5-20251101"), Some((5_000_000, 25_000_000)));
        assert_eq!(
            p("claude-opus-4-1-20250805"),
            Some((15_000_000, 75_000_000))
        );
        assert_eq!(p("claude-opus-4-20250514"), Some((15_000_000, 75_000_000)));
        assert_eq!(
            p("claude-sonnet-4-5-20250929"),
            Some((3_000_000, 15_000_000))
        );
        assert_eq!(p("claude-haiku-4-5"), Some((1_000_000, 5_000_000)));
        assert_eq!(p("claude-3-5-haiku-20241022"), Some((800_000, 4_000_000)));
        assert_eq!(p("claude-opus-5-5"), None, "newer than the table");
        assert_eq!(p("gpt-5"), None);
        assert_eq!(p(""), None);
        let o = price_for("claude-opus-4-5").unwrap();
        assert_eq!((o.cache_read(), o.cache_write()), (500_000, 6_250_000));
    }

    #[test]
    fn only_appended_bytes_are_read_and_a_partial_line_waits() {
        let first = line("m1", "claude-sonnet-4-5", NOW, (100, 0, 0, 0));
        let (t, _d, path) = tracker_with(&first);
        assert_eq!(t.refresh(DAY).unwrap().input_tokens, 100);
        assert_eq!(t.refresh(DAY), None, "nothing changed");

        // a half-written line is not consumed, the finished one is, exactly once
        let second = line("m2", "claude-sonnet-4-5", NOW, (50, 0, 0, 0));
        let (head, tail) = second.split_at(second.len() / 2);
        append(&path, head);
        assert_eq!(t.refresh(DAY), None);
        append(&path, tail);
        assert_eq!(t.refresh(DAY).unwrap().input_tokens, 150);

        // proof that the cache is used: the already-read bytes may change without being re-read
        let st_offset = t.lock().files.values().next().unwrap().offset;
        assert_eq!(st_offset, (first.len() + second.len()) as u64);
        let mut bytes = std::fs::read(&path).unwrap();
        bytes[2..6].copy_from_slice(b"XXXX"); // corrupt the first (already counted) line
        write_file(&path, std::str::from_utf8(&bytes).unwrap());
        append(&path, &line("m3", "claude-sonnet-4-5", NOW, (1, 0, 0, 0)));
        assert_eq!(t.refresh(DAY).unwrap().input_tokens, 151);
    }

    #[test]
    fn a_rewritten_shorter_file_is_read_from_the_start() {
        let (t, _d, path) = tracker_with(&line("m1", "claude-sonnet-4-5", NOW, (100, 0, 0, 0)));
        t.refresh(DAY).unwrap();
        write_file(&path, &line("n1", "claude-sonnet-4-5", NOW, (3, 0, 0, 0)));
        assert_eq!(t.refresh(DAY).unwrap().input_tokens, 3);
    }

    #[test]
    fn a_new_day_starts_from_zero() {
        let (t, _d, _p) = tracker_with(&line("m1", "claude-sonnet-4-5", NOW, (100, 0, 0, 0)));
        assert_eq!(t.refresh(DAY).unwrap().input_tokens, 100);
        let tomorrow = DayBounds {
            start_ms: DAY.end_ms,
            end_ms: DAY.end_ms + 86_400_000,
        };
        let u = t.refresh(tomorrow).unwrap();
        assert_eq!(u.total_tokens(), 0);
        assert_eq!(u.cost_micros, Some(0));
    }

    #[test]
    fn several_files_add_up_and_a_missing_one_is_skipped() {
        let (t, _d, path) = tracker_with(&line("a", "claude-sonnet-4-5", NOW, (100, 0, 0, 0)));
        let other = path.with_file_name("t.jsonl");
        write_file(&other, &line("b", "claude-haiku-4-5", NOW, (0, 40, 0, 0)));
        t.add(other);
        t.add(path.with_file_name("gone.jsonl"));
        let u = t.refresh(DAY).unwrap();
        assert_eq!((u.input_tokens, u.output_tokens), (100, 40));
        // 100 * $3/M + 40 * $5/M
        assert_eq!(u.cost_micros, Some(300 + 200));
    }

    #[test]
    fn refresh_scans_files_without_holding_the_tracker_lock() {
        let (t, _d, path) = tracker_with(&line("a", "claude-sonnet-4-5", NOW, (100, 0, 0, 0)));
        let late = path.with_file_name("late.jsonl");
        write_file(&late, &line("b", "claude-sonnet-4-5", NOW, (5, 0, 0, 0)));
        let scans = std::cell::Cell::new(0);
        let u = t
            .refresh_with(DAY, |p, st, day| {
                assert!(t.inner.try_lock().is_ok(), "the lock is held during a read");
                // the snapshot and the hook path answer mid-read
                assert_eq!(t.current().input_tokens, 0);
                t.add(late.clone());
                scans.set(scans.get() + 1);
                scan_file(p, st, day);
            })
            .unwrap();
        assert_eq!(
            u.input_tokens, 100,
            "the file read mid-refresh is merged back"
        );
        assert_eq!(scans.get(), 1);
        assert!(t.lock().files.contains_key(&path));
        // a file added mid-refresh is followed from the next refresh on
        assert_eq!(t.refresh(DAY).unwrap().input_tokens, 105);
    }

    #[test]
    fn nothing_followed_is_zero_dollars_and_not_a_change() {
        let t = UsageTracker::new();
        assert_eq!(t.current().cost_micros, Some(0));
        assert_eq!(t.refresh(DAY), None, "no event for an empty total");
    }

    #[test]
    fn add_asks_for_a_database_write_once_in_a_while() {
        let t = UsageTracker::new();
        let p = PathBuf::from("/x/s.jsonl");
        assert!(t.add(p.clone()));
        assert!(!t.add(p.clone()));
        assert!(t.add(PathBuf::from("/x/t.jsonl")));
    }

    #[test]
    fn local_day_bounds_contain_now_and_span_about_a_day() {
        for now in [1_793_613_600_000u64, 1_800_000_000_000, 1_711_846_800_000] {
            let d = DayBounds::local(now);
            assert!(d.start_ms <= now && now < d.end_ms, "{now} not in {d:?}");
            let hours = (d.end_ms - d.start_ms) / 3_600_000;
            assert!((23..=25).contains(&hours), "{hours}");
            assert_eq!(d.start_ms % 1000, 0);
        }
    }
}
