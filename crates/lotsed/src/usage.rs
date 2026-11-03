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
//! - Sub-agent transcripts count too: Claude Code writes them beside the session's, in
//!   `<session>/subagents/*.jsonl` (found from every followed transcript, read the same way). A
//!   message is counted once however many files hold it (a resumed session copies its history into a
//!   new file; the key is the message id and request id), across all followed files.
//! - Cost uses [`price_for`], a small built-in table (no network) keyed by model *family* and version,
//!   so provider id forms (`anthropic.claude-sonnet-4-5-20250929-v1:0`, `claude-sonnet-4-5@20250929`,
//!   a `[1m]` suffix) and dated ids price like the plain id. A model of a known family newer than the
//!   table (`claude-opus-5-9`) is priced as its family's newest known model: an estimate, never $0.
//!   A model of a family without a price makes the whole cost unknown (`cost_micros: None`,
//!   "tokens only"): the tokens still count.
//! - Cache writes cost 1.25 times the input price (5-minute entries) or twice (1-hour entries, split
//!   out of `usage.cache_creation`). Cache reads cost a tenth of it, except the 5.x generation:
//!   0.05x for Opus and Sonnet 5.5, 0.025x for Fable 5.1.
//!
//! Prices ($/MTok in/out) per Anthropic's price page as of 2026-11-03: Opus 5.5 4/20, Sonnet 5.5
//! 2/10, Haiku 5.5 0.10/0.50, Fable 5.1 10/50, Haiku 4.5 1/5. **Unverified:** the Haiku 5.5
//! cache-read rate (kept at 0.1x) and the pre-4.5 rows (written from memory).

use crate::transcript::parse_iso_utc_ms;
use lotse_proto::{UnixMs, UsageToday};
use serde_json::Value;
use std::collections::HashMap;
use std::io::{BufRead, BufReader, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::sync::{Condvar, Mutex, MutexGuard};
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
    /// What a prompt-cache read costs, in thousandths of the input price.
    pub cache_read_per_mille: u64,
}

/// The token counts of one message.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Counts {
    pub input: u64,
    pub output: u64,
    pub cache_read: u64,
    /// All cache writes, the 1-hour ones included.
    pub cache_write: u64,
    /// Of `cache_write`, the 1-hour entries.
    pub cache_write_1h: u64,
}

impl Counts {
    pub fn total(self) -> u64 {
        self.input + self.output + self.cache_read + self.cache_write
    }
}

impl Price {
    /// Prompt-cache reads cost a fraction of the input price, per model (see [`price_for`]).
    fn cache_read(self) -> u64 {
        self.input * self.cache_read_per_mille / 1000
    }

    /// Cache writes (5-minute entries) cost 1.25 times the input price.
    fn cache_write(self) -> u64 {
        self.input / 4 * 5
    }

    /// 1-hour cache entries cost twice the input price.
    fn cache_write_1h(self) -> u64 {
        self.input * 2
    }

    /// The cost of `c` in millionths of a dollar times a million (divide once, at the end).
    pub fn cost(self, c: Counts) -> u128 {
        let write_1h = c.cache_write_1h.min(c.cache_write);
        let write_5m = c.cache_write - write_1h;
        u128::from(c.input) * u128::from(self.input)
            + u128::from(c.output) * u128::from(self.output)
            + u128::from(c.cache_read) * u128::from(self.cache_read())
            + u128::from(write_5m) * u128::from(self.cache_write())
            + u128::from(write_1h) * u128::from(self.cache_write_1h())
    }
}

const fn usd(dollars_x100: u64) -> u64 {
    dollars_x100 * 10_000
}

/// `model` as `claude-<family>-<version>`: provider prefixes (`anthropic.`, `us.anthropic.`,
/// `publishers/anthropic/models/`), a Vertex `@date`, a Bedrock `-v1:0`, a `[1m]` suffix, a date and
/// `-latest` taken off, lower case. A string without `claude-` comes back lower-cased as it is.
fn canonical_model(model: &str) -> String {
    let lower = model.trim().to_ascii_lowercase();
    let from = lower.find("claude-").unwrap_or(0);
    let mut id = &lower[from..];
    if let Some(at) = id.find(['@', ':', '[']) {
        id = &id[..at];
    }
    loop {
        let before = id;
        id = id.strip_suffix("-latest").unwrap_or(id);
        if let Some((rest, v)) = id.rsplit_once("-v")
            && !v.is_empty()
            && v.bytes().all(|b| b.is_ascii_digit())
        {
            id = rest;
        }
        if let Some((rest, date)) = id.rsplit_once('-')
            && date.len() == 8
            && date.bytes().all(|b| b.is_ascii_digit())
        {
            id = rest;
        }
        if id == before {
            return id.to_owned();
        }
    }
}

/// The price of `model`: by family and version (see the module docs), `None` for a family without
/// a price. A version newer than the table is priced as the family's newest known model.
pub fn price_for(model: &str) -> Option<Price> {
    let id = canonical_model(model);
    let mut parts = id.strip_prefix("claude-")?.split('-');
    let (mut family, mut version) = (None, Vec::new());
    for part in &mut parts {
        match part {
            "opus" | "sonnet" | "haiku" | "fable" => family = Some(part),
            n if n.len() <= 2 && !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit()) => {
                version.push(n.parse::<u32>().ok()?);
            }
            _ => return None,
        }
    }
    // `claude-opus-4` is 4.0; `claude-3-5-haiku` is 3.5
    let version = (
        version.first().copied().unwrap_or(0),
        version.get(1).copied().unwrap_or(0),
    );
    // (input, output) in dollars x 100 per million tokens, cache read in thousandths of input
    let (input, output, cache_read_per_mille) = match family? {
        "fable" if version >= (5, 0) => (1000, 5000, 25),
        "opus" if version >= (5, 0) => (400, 2000, 50),
        "opus" if version >= (4, 5) => (500, 2500, 100),
        "opus" => (1500, 7500, 100),
        "sonnet" if version >= (5, 0) => (200, 1000, 50),
        "sonnet" => (300, 1500, 100),
        "haiku" if version >= (5, 0) => (10, 50, 100),
        "haiku" if version >= (4, 5) => (100, 500, 100),
        "haiku" if version >= (3, 5) => (80, 400, 100),
        "haiku" => (25, 125, 100),
        _ => return None,
    };
    Some(Price {
        input: usd(input),
        output: usd(output),
        cache_read_per_mille,
    })
}

#[derive(Debug, Clone, Default)]
struct Entry {
    model: String,
    counts: Counts,
}

#[derive(Debug, Default)]
struct FileState {
    mtime: Option<SystemTime>,
    len: u64,
    /// Bytes consumed: up to and including the last complete line read.
    offset: u64,
    entries: HashMap<String, Entry>,
}

/// Today's total over every followed file, a message counted once.
#[derive(Debug, Default)]
struct Sum {
    counts: Counts,
    /// Cost in millionths of a dollar times a million (divide once, at the end: no rounding drift).
    cost: u128,
    /// A message of a family without a price took part.
    unpriced: bool,
}

fn sum_of<'a>(files: impl Iterator<Item = &'a FileState>) -> Sum {
    // the same message in two files (a resumed session copies its history) counts once
    let mut messages: HashMap<&str, &Entry> = HashMap::new();
    for st in files {
        for (key, entry) in &st.entries {
            messages.insert(key, entry);
        }
    }
    let mut sum = Sum::default();
    for e in messages.values() {
        let c = e.counts;
        sum.counts.input += c.input;
        sum.counts.output += c.output;
        sum.counts.cache_read += c.cache_read;
        sum.counts.cache_write += c.cache_write;
        match price_for(&e.model) {
            Some(p) => sum.cost += p.cost(c),
            None => sum.unpriced = true,
        }
    }
    sum
}

/// Claude Code's sub-agent transcripts of a session transcript: `<session>/subagents/*.jsonl` beside
/// `<session>.jsonl`. Regular files only (a link is not followed), at most [`MAX_SUBAGENT_FILES`].
fn subagent_files(transcript: &Path) -> Vec<PathBuf> {
    let dir = transcript.with_extension("").join("subagents");
    let Ok(entries) = std::fs::read_dir(&dir) else {
        return Vec::new();
    };
    entries
        .filter_map(Result::ok)
        .filter(|e| e.file_type().is_ok_and(|t| t.is_file()))
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|x| x == "jsonl"))
        .take(MAX_SUBAGENT_FILES)
        .collect()
}

/// Most sub-agent transcripts followed per session transcript.
const MAX_SUBAGENT_FILES: usize = 512;

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
    /// Set by [`UsageTracker::nudge`], waited on by the thread that refreshes.
    nudged: (Mutex<bool>, Condvar),
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
            nudged: (Mutex::new(false), Condvar::new()),
        }
    }

    /// Something worth reading happened (a hook of a turn that ended): wake the refreshing thread
    /// instead of leaving the total to its next periodic read.
    pub fn nudge(&self) {
        *self.nudged.0.lock().unwrap_or_else(|e| e.into_inner()) = true;
        self.nudged.1.notify_all();
    }

    /// Wait up to `timeout` for a [`Self::nudge`]; `true` when there was one (it is consumed).
    pub fn wait_nudge(&self, timeout: Duration) -> bool {
        let guard = self.nudged.0.lock().unwrap_or_else(|e| e.into_inner());
        let (mut guard, _) = self
            .nudged
            .1
            .wait_timeout_while(guard, timeout, |nudged| !*nudged)
            .unwrap_or_else(|e| e.into_inner());
        std::mem::take(&mut *guard)
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
        // the sub-agent transcripts beside each followed one (a file stat when there are none)
        let known: std::collections::HashSet<PathBuf> =
            work.iter().map(|(path, _)| path.clone()).collect();
        let found: Vec<PathBuf> = work
            .iter()
            .flat_map(|(path, _)| subagent_files(path))
            .filter(|p| !known.contains(p))
            .collect();
        work.extend(found.into_iter().map(|p| (p, FileState::default())));
        for (path, st) in &mut work {
            scan(path, st, day);
        }
        let mut g = self.lock();
        for (path, st) in work {
            g.files.insert(path, st);
        }
        let total = sum_of(g.files.values());
        let usage = UsageToday {
            input_tokens: total.counts.input,
            output_tokens: total.counts.output,
            cache_read_tokens: total.counts.cache_read,
            cache_write_tokens: total.counts.cache_write,
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
        counts: Counts {
            input: n("input_tokens"),
            output: n("output_tokens"),
            cache_read: n("cache_read_input_tokens"),
            cache_write: n("cache_creation_input_tokens"),
            cache_write_1h: usage
                .get("cache_creation")
                .and_then(|c| c.get("ephemeral_1h_input_tokens"))
                .and_then(Value::as_u64)
                .unwrap_or(0),
        },
    };
    if entry.counts.total() == 0 {
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
        text += &line("b", "claude-zephyr-9-9", NOW, (500, 0, 0, 0));
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
        assert_eq!(p("claude-3-haiku-20240307"), Some((250_000, 1_250_000)));
        assert_eq!(p("claude-3-opus-20240229"), Some((15_000_000, 75_000_000)));
        assert_eq!(
            p("claude-3-7-sonnet-20250219"),
            Some((3_000_000, 15_000_000))
        );
        assert_eq!(p("gpt-5"), None);
        assert_eq!(p(""), None);
        assert_eq!(p("<synthetic>"), None);
        let o = price_for("claude-opus-4-5").unwrap();
        assert_eq!(
            (o.cache_read(), o.cache_write(), o.cache_write_1h()),
            (500_000, 6_250_000, 10_000_000)
        );
    }

    #[test]
    fn models_newer_than_the_table_are_priced_as_their_family_never_as_zero() {
        // (documented estimates: the family's newest known model)
        let p = |m: &str| price_for(m).map(|p| (p.input, p.output));
        assert_eq!(p("claude-opus-5-5"), Some((4_000_000, 20_000_000)));
        assert_eq!(p("claude-opus-4-6"), Some((5_000_000, 25_000_000)));
        assert_eq!(p("claude-opus-5-9"), Some((4_000_000, 20_000_000)));
        assert_eq!(p("claude-fable-5-9"), Some((10_000_000, 50_000_000)));
        // a family without a price: unknown, not $0
        assert_eq!(p("claude-zephyr-5-1"), None);
    }

    #[test]
    fn the_5_x_models_have_their_own_prices_and_cache_read_rates() {
        let p = |m: &str| {
            price_for(m).map(|p| {
                (
                    p.input,
                    p.output,
                    p.cache_read(),
                    p.cache_write(),
                    p.cache_write_1h(),
                )
            })
        };
        // $/MTok in/out; cache reads 0.05x (Opus, Sonnet 5.5), 0.025x (Fable 5.1), 0.1x otherwise
        assert_eq!(
            p("claude-opus-5-5"),
            Some((4_000_000, 20_000_000, 200_000, 5_000_000, 8_000_000))
        );
        assert_eq!(
            p("claude-sonnet-5-5"),
            Some((2_000_000, 10_000_000, 100_000, 2_500_000, 4_000_000))
        );
        assert_eq!(
            p("claude-haiku-5-5"),
            Some((100_000, 500_000, 10_000, 125_000, 200_000))
        );
        assert_eq!(
            p("claude-fable-5-1"),
            Some((10_000_000, 50_000_000, 250_000, 12_500_000, 20_000_000))
        );
        assert_eq!(
            p("claude-haiku-4-5"),
            Some((1_000_000, 5_000_000, 100_000, 1_250_000, 2_000_000))
        );
    }

    #[test]
    fn provider_id_forms_and_dates_price_like_the_plain_id() {
        let sonnet = Some((3_000_000, 15_000_000));
        for id in [
            "claude-sonnet-4-5",
            "claude-sonnet-4-5-20250929",
            "anthropic.claude-sonnet-4-5-20250929-v1:0",
            "us.anthropic.claude-sonnet-4-5-20250929-v1:0",
            "global.anthropic.claude-sonnet-4-5-20250929-v1:0",
            "eu.anthropic.claude-sonnet-4-5-v1:0",
            "claude-sonnet-4-5@20250929",
            "publishers/anthropic/models/claude-sonnet-4-5@20250929",
            "claude-sonnet-4-5[1m]",
            "Claude-Sonnet-4-5",
            "claude-sonnet-4-5-latest",
        ] {
            assert_eq!(price_for(id).map(|p| (p.input, p.output)), sonnet, "{id}");
        }
        assert_eq!(
            price_for("anthropic.claude-opus-4-1-20250805-v1:0").map(|p| p.input),
            Some(15_000_000)
        );
        assert_eq!(
            price_for("claude-haiku-4-5@20251001").map(|p| p.input),
            Some(1_000_000)
        );
        assert_eq!(
            canonical_model("us.anthropic.claude-opus-4-5-20251101-v1:0"),
            "claude-opus-4-5"
        );
    }

    #[test]
    fn one_hour_cache_writes_cost_twice_the_input_price() {
        // sonnet: $3 in; 1M 5-minute writes $3.75, 1M 1-hour writes $6
        let usage = r#""usage":{"input_tokens":0,"output_tokens":0,"cache_read_input_tokens":0,"cache_creation_input_tokens":2000000,"cache_creation":{"ephemeral_5m_input_tokens":1000000,"ephemeral_1h_input_tokens":1000000}}"#;
        let text = format!(
            "{{\"type\":\"assistant\",\"timestamp\":\"{NOW}\",\"requestId\":\"r1\",\"message\":{{\"id\":\"m1\",\"model\":\"claude-sonnet-4-5\",{usage}}}}}\n"
        );
        let (t, _d, _p) = tracker_with(&text);
        let u = t.refresh(DAY).unwrap();
        assert_eq!(u.cache_write_tokens, 2_000_000);
        assert_eq!(u.cost_micros, Some(3_750_000 + 6_000_000));
    }

    #[test]
    fn sub_agent_transcripts_beside_the_session_transcript_count_once() {
        let (t, _d, path) =
            tracker_with(&line("lead", "claude-haiku-4-5", NOW, (1_000_000, 0, 0, 0)));
        let subs = path.with_extension("").join("subagents");
        std::fs::create_dir_all(&subs).unwrap();
        // two sub-agents on sonnet; one of their messages is also in the lead's file (a copy)
        let a = subs.join("agent-a1.jsonl");
        let b = subs.join("agent-b2.jsonl");
        write_file(
            &a,
            &(line("s1", "claude-sonnet-4-5", NOW, (1_000_000, 0, 0, 0))
                + &line("lead", "claude-haiku-4-5", NOW, (1_000_000, 0, 0, 0))),
        );
        write_file(
            &b,
            &line("s2", "claude-sonnet-4-5", NOW, (0, 100_000, 0, 0)),
        );
        std::fs::write(subs.join("notes.txt"), "x").unwrap(); // not a transcript
        let u = t.refresh(DAY).unwrap();
        assert_eq!((u.input_tokens, u.output_tokens), (2_000_000, 100_000));
        // haiku $1 + sonnet $3 + sonnet 0.1M out $1.5
        assert_eq!(u.cost_micros, Some(1_000_000 + 3_000_000 + 1_500_000));
        // a sub-agent that writes later is picked up by the next refresh
        append(&b, &line("s3", "claude-sonnet-4-5", NOW, (10, 0, 0, 0)));
        assert_eq!(t.refresh(DAY).unwrap().input_tokens, 2_000_010);
        assert_eq!(t.refresh(DAY), None);
    }

    #[test]
    fn the_same_message_in_two_followed_transcripts_counts_once() {
        // a resumed session copies the old history into its new file
        let (t, _d, path) = tracker_with(&line("m1", "claude-sonnet-4-5", NOW, (100, 0, 0, 0)));
        let resumed = path.with_file_name("resumed.jsonl");
        write_file(
            &resumed,
            &(line("m1", "claude-sonnet-4-5", NOW, (100, 0, 0, 0))
                + &line("m2", "claude-sonnet-4-5", NOW, (7, 0, 0, 0))),
        );
        t.add(resumed);
        assert_eq!(t.refresh(DAY).unwrap().input_tokens, 107);
    }

    #[test]
    fn an_unpriced_family_keeps_its_tokens_and_the_priced_part_of_the_cost() {
        let mut text = line("a", "claude-sonnet-4-5", NOW, (1_000_000, 0, 0, 0));
        text += &line("b", "claude-zephyr-5-1", NOW, (500_000, 0, 0, 0));
        let (t, _d, _p) = tracker_with(&text);
        let u = t.refresh(DAY).unwrap();
        assert_eq!((u.input_tokens, u.cost_micros), (1_500_000, None));
        assert_eq!(t.priced_micros(), 3_000_000);
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
