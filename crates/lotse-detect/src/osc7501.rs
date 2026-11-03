//! OSC 7501 ("program status") scanner, parser and adapter. Source: [`DetectSource::Osc7501`].
//!
//! A program reports its own state to the terminal with `ESC ] 7501 ; key=value : key=value … ST`
//! (`ST` is `ESC \` or `BEL`). The daemon feeds every PTY output chunk to an [`Osc7501Scanner`], wraps
//! each body it returns in an [`OscObservation`] (the scanner does that), and offers it to the engine;
//! the [`Osc7501Adapter`] turns the reports into a status. The raw bytes still go to the ring buffer
//! and xterm.js, which ignore the unknown OSC.
//!
//! # Grammar
//!
//! Keys: `state` (`idle working done blocked error clear`, required), `id` (absent = the program's
//! root record; `a/b/c` = a record beneath it, up to 8 segments of `[A-Za-z0-9_.+-]{1,32}`), `kind`
//! (`permission question auth`, only with `blocked`), `progress` (0-100, only with `working` and
//! `blocked`), `app` (`[A-Za-z0-9_.+-]+`, at most 32 bytes), `title` and `msg` (base64 of UTF-8 text
//! without control characters; at most 192 and 2048 bytes decoded). A whole sequence is at most 4096
//! bytes. A report with no `state`, a bad `id`, or an over-limit field is ignored whole; a malformed
//! pair elsewhere is skipped. `?` alone is a capability query: a terminal that supports the protocol
//! answers [`QUERY_REPLY`]; a program that gets no answer may not report at all, so the daemon should
//! answer ([`is_query`]).
//!
//! **Source and verification.** Ported from Orca's reader (`src/shared/program-status/protocol.ts`,
//! `agent-status-projector.ts`, `osc-scanner.ts` in `/Users/naim/Repos/research/orca`, read
//! 2026-11-02), which cites the spec at <https://www.superlogical.com/rex/docs/build/program-status>.
//! **Unverified:** no web access, so the spec page itself was not read; check the grammar there before
//! release. **Unverified:** which agents emit it. Orca's recorded Codex 0.158.0 transcripts contain no
//! OSC 7501, so Codex status comes from the screen adapter; this adapter is the stronger source for
//! any program that does report.
//!
//! # Status table (root record = the session, `id` records = sub-agents)
//!
//! | Report | Effect |
//! |---|---|
//! | root `working` | `Run`; sub-agents that finished in the previous turn are dropped |
//! | root `blocked` | `Perm` (`PermRequest { tool: kind, summary: msg }`) |
//! | root `done` / `error` | `Done` / `Fail` (reported `Run` while a sub-agent still runs) |
//! | root `idle` | `Idle`; after `done` / `error` of the same invocation the result is kept |
//! | root `clear` | everything cleared (`Unknown`, no sub-agents) |
//! | child `working` / `blocked` / `idle` / `done` / `error` | sub-agent `Run` / `Perm` / `Idle` / `Done` / `Fail`; a blocked child makes the session `Perm`; a report that omits `title` / `app` keeps the earlier ones; at most 256 records (the oldest goes) |
//! | child `clear` | that sub-agent and everything beneath it (`a/…`) removed |
//! | PTY child exited, or a shell took the foreground | everything cleared (`Unknown`) |

use crate::adapter::{Adapter, Detection, Observation, OscObservation};
use lotse_proto::{
    AgentKind, DetectSource, PermRequest, SessionId, Status, SubAgent, SubAgentId, UnixMs,
};

/// `ESC ] 7501 ;`
pub const OSC_PREFIX: &[u8] = b"\x1b]7501;";

/// What a terminal that supports the protocol writes back to a `?` query.
pub const QUERY_REPLY: &[u8] =
    b"\x1b]7501;?:states=idle,working,done,blocked,error:kinds=permission,question,auth\x1b\\";

/// Whole sequence cap, `OSC` through `ST`.
pub const MAX_SEQUENCE_BYTES: usize = 4096;

const MAX_KEY_BYTES: usize = 16;
const MAX_MSG_ENCODED: usize = 2732;
const MAX_MSG_DECODED: usize = 2048;
const MAX_TITLE_ENCODED: usize = 256;
const MAX_TITLE_DECODED: usize = 192;
const MAX_ID_LENGTH: usize = 128;
const MAX_ID_DEPTH: usize = 8;
const MAX_APP_BYTES: usize = 32;

/// Longest `PermRequest::summary`, in characters.
const SUMMARY_MAX_CHARS: usize = 160;
/// Sub-agent records kept per session (Orca's `PROGRAM_STATUS_MAX_RECORDS`).
const MAX_RECORDS: usize = 256;

const ESC: u8 = 0x1b;
const BEL: u8 = 0x07;

/// Called by `Engine::with_default_adapters`: any agent may report, so every agent kind but a plain
/// terminal gets the adapter.
pub fn default_adapter(_session: &SessionId, agent: AgentKind) -> Option<Box<dyn Adapter>> {
    (agent != AgentKind::Terminal).then(|| Box::new(Osc7501Adapter::new()) as Box<dyn Adapter>)
}

// ---- scanner ------------------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
enum ScanState {
    #[default]
    Ground,
    /// This many bytes of [`OSC_PREFIX`] matched so far.
    Prefix(usize),
    Body,
    /// Saw `ESC` inside a body: `\` ends it, anything else cancels it.
    BodyEsc,
}

/// Streaming scanner for OSC 7501 sequences in PTY output. Chunk boundaries may fall anywhere,
/// including inside the prefix or the terminator.
#[derive(Debug, Default)]
pub struct Osc7501Scanner {
    state: ScanState,
    body: Vec<u8>,
}

impl Osc7501Scanner {
    pub fn new() -> Self {
        Self::default()
    }

    /// Feed one PTY output chunk; returns one observation per complete sequence, in order. `at` is
    /// the time of the chunk.
    pub fn feed(&mut self, chunk: &[u8], at: UnixMs) -> Vec<OscObservation> {
        let mut out = Vec::new();
        for &byte in chunk {
            self.step(byte, at, &mut out);
        }
        out
    }

    fn step(&mut self, byte: u8, at: UnixMs, out: &mut Vec<OscObservation>) {
        match self.state {
            ScanState::Ground => {
                if byte == ESC {
                    self.state = ScanState::Prefix(1);
                }
            }
            ScanState::Prefix(matched) => {
                if byte == OSC_PREFIX[matched] {
                    if matched + 1 == OSC_PREFIX.len() {
                        self.body.clear();
                        self.state = ScanState::Body;
                    } else {
                        self.state = ScanState::Prefix(matched + 1);
                    }
                } else if byte == ESC {
                    self.state = ScanState::Prefix(1);
                } else {
                    self.state = ScanState::Ground;
                }
            }
            ScanState::Body => match byte {
                BEL => self.finish(at, out),
                ESC => self.state = ScanState::BodyEsc,
                _ if self.body.len() >= MAX_SEQUENCE_BYTES => self.abort(),
                _ => self.body.push(byte),
            },
            ScanState::BodyEsc => {
                if byte == b'\\' {
                    self.finish(at, out);
                } else {
                    // The sequence was cancelled; the ESC may start a new one, so this byte is its
                    // second byte.
                    self.abort();
                    self.state = ScanState::Prefix(1);
                    self.step(byte, at, out);
                }
            }
        }
    }

    fn finish(&mut self, at: UnixMs, out: &mut Vec<OscObservation>) {
        self.state = ScanState::Ground;
        // Bodies are ASCII by grammar; anything else is not a report.
        if let Ok(payload) = String::from_utf8(std::mem::take(&mut self.body)) {
            out.push(OscObservation { payload, at });
        }
    }

    fn abort(&mut self) {
        self.state = ScanState::Ground;
        self.body.clear();
    }
}

// ---- parser -------------------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReportState {
    Idle,
    Working,
    Done,
    Blocked,
    Error,
    Clear,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReportKind {
    Permission,
    Question,
    Auth,
}

impl ReportKind {
    fn as_str(self) -> &'static str {
        match self {
            ReportKind::Permission => "permission",
            ReportKind::Question => "question",
            ReportKind::Auth => "auth",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Report {
    /// `None` addresses the root record.
    pub id: Option<String>,
    pub state: ReportState,
    pub kind: Option<ReportKind>,
    pub progress: Option<u8>,
    pub app: Option<String>,
    pub title: Option<String>,
    pub msg: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Message {
    Query,
    Report(Report),
}

/// Whether an [`OscObservation::payload`] is a capability query the terminal should answer with
/// [`QUERY_REPLY`].
pub fn is_query(payload: &str) -> bool {
    payload.trim() == "?"
}

fn is_key(s: &str) -> bool {
    !s.is_empty() && s.bytes().all(|b| b.is_ascii_lowercase())
}

fn is_plain_value(s: &str) -> bool {
    s.bytes()
        .all(|b| b.is_ascii_alphanumeric() || b"_.,+/=-".contains(&b))
}

fn is_segment(s: &str) -> bool {
    (1..=32).contains(&s.len())
        && s.bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"_.+-".contains(&b))
}

fn is_valid_id(id: &str) -> bool {
    id.len() <= MAX_ID_LENGTH && {
        let segments: Vec<&str> = id.split('/').collect();
        segments.len() <= MAX_ID_DEPTH && segments.iter().all(|s| is_segment(s))
    }
}

fn base64_value(b: u8) -> Option<u8> {
    match b {
        b'A'..=b'Z' => Some(b - b'A'),
        b'a'..=b'z' => Some(b - b'a' + 26),
        b'0'..=b'9' => Some(b - b'0' + 52),
        b'+' => Some(62),
        b'/' => Some(63),
        _ => None,
    }
}

/// Decoded single-line text, or `None` when the report must be discarded whole.
fn decode_text(encoded: &str, max_encoded: usize, max_decoded: usize) -> Option<String> {
    // Why before decoding: the encoded cap bounds the work a hostile program can cause.
    if encoded.len() > max_encoded {
        return None;
    }
    let unpadded = encoded.trim_end_matches('=');
    if encoded.len() - unpadded.len() > 2 || unpadded.len() % 4 == 1 {
        return None;
    }
    let mut bytes = Vec::with_capacity(unpadded.len() * 3 / 4);
    let (mut acc, mut bits) = (0u32, 0u32);
    for b in unpadded.bytes() {
        acc = (acc << 6) | u32::from(base64_value(b)?);
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            bytes.push((acc >> bits) as u8);
            acc &= (1 << bits) - 1;
        }
    }
    if bytes.len() > max_decoded {
        return None;
    }
    let text = String::from_utf8(bytes).ok()?;
    // The spec refuses decoded C0, DEL and C1 controls.
    let has_control = text
        .chars()
        .any(|c| matches!(c, '\u{0}'..='\u{1f}' | '\u{7f}'..='\u{9f}'));
    (!has_control).then_some(text)
}

/// Parse the body between `OSC 7501 ;` and `ST`. `None` means ignore.
pub fn parse_body(body: &str) -> Option<Message> {
    if body.len() > MAX_SEQUENCE_BYTES {
        return None;
    }
    if is_query(body) {
        return Some(Message::Query);
    }
    let mut pairs: Vec<(&str, &str)> = Vec::new();
    let (mut msg, mut title) = (None, None);
    for pair in body.split(':') {
        let Some((key, value)) = pair.split_once('=') else {
            continue;
        };
        let (key, value) = (key.trim(), value.trim());
        // A hard cap broken anywhere voids the report, even if a duplicate follows.
        if key.len() > MAX_KEY_BYTES
            || (key == "app" && value.len() > MAX_APP_BYTES)
            || (key == "id" && !is_valid_id(value))
        {
            return None;
        }
        // Malformed pairs are skipped; the rest of the report still applies.
        if !is_key(key) || !is_plain_value(value) {
            continue;
        }
        match key {
            "msg" => msg = Some(decode_text(value, MAX_MSG_ENCODED, MAX_MSG_DECODED)?),
            "title" => title = Some(decode_text(value, MAX_TITLE_ENCODED, MAX_TITLE_DECODED)?),
            _ => pairs.push((key, value)),
        }
    }
    // Last occurrence wins.
    let get = |key: &str| pairs.iter().rev().find(|(k, _)| *k == key).map(|(_, v)| *v);

    let state = match get("state")? {
        "idle" => ReportState::Idle,
        "working" => ReportState::Working,
        "done" => ReportState::Done,
        "blocked" => ReportState::Blocked,
        "error" => ReportState::Error,
        "clear" => ReportState::Clear,
        _ => return None,
    };
    let kind = match (state, get("kind")) {
        (ReportState::Blocked, Some("permission")) => Some(ReportKind::Permission),
        (ReportState::Blocked, Some("question")) => Some(ReportKind::Question),
        (ReportState::Blocked, Some("auth")) => Some(ReportKind::Auth),
        _ => None,
    };
    let progress = get("progress")
        .filter(|p| {
            matches!(state, ReportState::Working | ReportState::Blocked)
                && (1..=3).contains(&p.len())
                && p.bytes().all(|b| b.is_ascii_digit())
        })
        .and_then(|p| p.parse::<u8>().ok().filter(|p| *p <= 100));
    let app = get("app")
        .filter(|a| {
            !a.is_empty()
                && a.bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b"_.+-".contains(&b))
        })
        .map(str::to_owned);
    Some(Message::Report(Report {
        id: get("id").map(str::to_owned),
        state,
        kind,
        progress,
        app,
        title: title.filter(|t| !t.is_empty()),
        msg: msg.filter(|m| !m.is_empty()),
    }))
}

// ---- adapter ------------------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq)]
struct Record {
    state: ReportState,
    kind: Option<ReportKind>,
    title: Option<String>,
    msg: Option<String>,
    app: Option<String>,
}

impl Record {
    fn status(&self) -> Status {
        match self.state {
            ReportState::Working => Status::Run,
            ReportState::Blocked => Status::Perm,
            ReportState::Done => Status::Done,
            ReportState::Error => Status::Fail,
            ReportState::Idle => Status::Idle,
            ReportState::Clear => Status::Unknown,
        }
    }

    fn perm(&self) -> PermRequest {
        let summary = self.msg.as_deref().or(self.title.as_deref()).unwrap_or("");
        PermRequest {
            tool: self.kind.map_or("blocked", ReportKind::as_str).to_owned(),
            summary: summary.chars().take(SUMMARY_MAX_CHARS).collect(),
            always: None,
            id: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
struct Reported {
    status: Status,
    perm: Option<PermRequest>,
    sub_agents: Vec<SubAgent>,
}

/// OSC 7501 adapter for one session.
#[derive(Debug, Default)]
pub struct Osc7501Adapter {
    root: Option<Record>,
    /// Insertion order is display order.
    children: Vec<(String, Record)>,
    reported: Option<Reported>,
}

impl Osc7501Adapter {
    pub fn new() -> Self {
        Self::default()
    }

    fn apply(&mut self, report: Report) {
        let Report {
            id,
            state,
            kind,
            app,
            title,
            msg,
            ..
        } = report;
        // `title` and `app` name the record, so a report that omits them keeps the earlier ones;
        // `msg` and `kind` describe the current state and do not. (Deliberately unlike Orca's record
        // set, which replaces a record whole: Orca drops finished children, Lotse lists them as done
        // and needs their names.)
        let previous = match &id {
            None => self.root.as_ref(),
            Some(id) => self.children.iter().find(|(c, _)| c == id).map(|(_, r)| r),
        };
        let record = Record {
            state,
            kind,
            title: title.or_else(|| previous.and_then(|p| p.title.clone())),
            msg,
            app: app.or_else(|| previous.and_then(|p| p.app.clone())),
        };
        match id {
            None => self.apply_root(record),
            // Clearing a record clears everything beneath it (`a` takes `a/b` with it).
            Some(id) if state == ReportState::Clear => self
                .children
                .retain(|(c, _)| !(*c == id || c.starts_with(&format!("{id}/")))),
            Some(id) => match self.children.iter_mut().find(|(c, _)| *c == id) {
                Some((_, slot)) => *slot = record,
                None => {
                    // A runaway program must not grow memory: past the cap the oldest record goes.
                    if self.children.len() >= MAX_RECORDS {
                        self.children.remove(0);
                    }
                    self.children.push((id, record));
                }
            },
        }
    }

    fn apply_root(&mut self, record: Record) {
        match record.state {
            // Clearing the root clears the whole program, children included.
            ReportState::Clear => self.reset(),
            // Idle follows startup work or a finished turn: it proves rest, not cancellation, so an
            // unseen result of the same invocation stays.
            ReportState::Idle
                if matches!(
                    self.root.as_ref().map(|r| r.state),
                    Some(ReportState::Done | ReportState::Error)
                ) => {}
            ReportState::Working => {
                // A new turn: sub-agents that finished in the previous one go.
                if self.root.as_ref().map(|r| r.state) != Some(ReportState::Working) {
                    self.children.retain(|(_, c)| {
                        matches!(c.state, ReportState::Working | ReportState::Blocked)
                    });
                }
                self.root = Some(record);
            }
            _ => self.root = Some(record),
        }
    }

    fn reset(&mut self) {
        self.root = None;
        self.children.clear();
    }

    fn derive(&self) -> Reported {
        let sub_agents: Vec<SubAgent> = self
            .children
            .iter()
            .map(|(id, c)| SubAgent {
                id: SubAgentId::new(id.as_str()),
                name: c
                    .title
                    .clone()
                    .or_else(|| c.msg.clone())
                    .or_else(|| c.app.clone())
                    .unwrap_or_else(|| id.rsplit('/').next().unwrap_or(id).to_owned()),
                status: c.status(),
            })
            .collect();
        let root_status = self.root.as_ref().map(Record::status);
        let blocked = self
            .root
            .iter()
            .chain(self.children.iter().map(|(_, c)| c))
            .find(|r| r.state == ReportState::Blocked);
        let child_running = self
            .children
            .iter()
            .any(|(_, c)| c.state == ReportState::Working);
        let status = if blocked.is_some() {
            Status::Perm
        } else if root_status == Some(Status::Fail) {
            Status::Fail
        } else if root_status == Some(Status::Run) || child_running {
            Status::Run
        } else {
            root_status.unwrap_or(Status::Unknown)
        };
        Reported {
            status,
            perm: blocked.filter(|_| status == Status::Perm).map(Record::perm),
            sub_agents,
        }
    }

    fn detection(&mut self, at: UnixMs) -> Vec<Detection> {
        let now = self.derive();
        let nothing_yet =
            self.reported.is_none() && now.status == Status::Unknown && now.sub_agents.is_empty();
        if nothing_yet || self.reported.as_ref() == Some(&now) {
            return Vec::new();
        }
        let detection = Detection {
            source: DetectSource::Osc7501,
            status: Some(now.status),
            sub_agents: Some(now.sub_agents.clone()),
            perm: now.perm.clone(),
            at,
        };
        self.reported = Some(now);
        vec![detection]
    }
}

impl Adapter for Osc7501Adapter {
    fn name(&self) -> &'static str {
        "osc7501"
    }

    fn agent(&self) -> AgentKind {
        // Any agent can report; the engine does not route by this.
        AgentKind::Terminal
    }

    fn source(&self) -> DetectSource {
        DetectSource::Osc7501
    }

    fn observe(&mut self, obs: &Observation) -> Vec<Detection> {
        match obs {
            Observation::Osc(osc) => match parse_body(&osc.payload) {
                Some(Message::Report(report)) => {
                    self.apply(report);
                    self.detection(osc.at)
                }
                Some(Message::Query) | None => Vec::new(),
            },
            Observation::Process(p)
                if p.foreground.as_deref().is_none_or(crate::screen::is_shell) =>
            {
                self.reset();
                self.detection(p.at)
            }
            _ => Vec::new(),
        }
    }

    /// All reports of one chunk are applied first, then one detection of the final state.
    // TODO(osc-batch): a chunk that holds a whole `working` → `done` turn reports only `done`, so
    // that turn's "finished" banner is skipped; revisit if an agent is seen to emit both at once.
    fn observe_batch(&mut self, batch: &[Observation]) -> Vec<Detection> {
        if !batch.iter().all(|o| matches!(o, Observation::Osc(_))) {
            return batch.iter().flat_map(|obs| self.observe(obs)).collect();
        }
        let mut at = None;
        for obs in batch {
            if let Observation::Osc(osc) = obs
                && let Some(Message::Report(report)) = parse_body(&osc.payload)
            {
                self.apply(report);
                at = Some(osc.at);
            }
        }
        at.map_or_else(Vec::new, |at| self.detection(at))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::adapter::ProcessObservation;

    fn bodies(scanner: &mut Osc7501Scanner, chunk: &[u8]) -> Vec<String> {
        scanner
            .feed(chunk, 0)
            .into_iter()
            .map(|o| o.payload)
            .collect()
    }

    fn b64(text: &str) -> String {
        const T: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
        let mut out = String::new();
        for chunk in text.as_bytes().chunks(3) {
            let n = chunk
                .iter()
                .enumerate()
                .fold(0u32, |n, (i, b)| n | u32::from(*b) << (16 - 8 * i));
            for i in 0..=chunk.len() {
                out.push(T[(n >> (18 - 6 * i) & 63) as usize] as char);
            }
            out.extend(std::iter::repeat_n('=', 3 - chunk.len()));
        }
        out
    }

    fn osc(adapter: &mut Osc7501Adapter, body: &str, at: UnixMs) -> Option<(Status, usize)> {
        let det = adapter.observe(&Observation::Osc(OscObservation {
            payload: body.into(),
            at,
        }));
        det.last()
            .map(|d| (d.status.unwrap(), d.sub_agents.as_ref().unwrap().len()))
    }

    #[test]
    fn a_batch_of_reports_yields_one_detection_of_the_final_state() {
        let mut a = Osc7501Adapter::new();
        let mut batch = Vec::new();
        for n in 0..400u64 {
            let state = if n % 2 == 0 { "working" } else { "blocked" };
            batch.push(Observation::Osc(OscObservation {
                payload: format!("state={state}"),
                at: n,
            }));
        }
        batch.push(Observation::Osc(OscObservation {
            payload: "state=blocked".into(),
            at: 400,
        }));
        let det = a.observe_batch(&batch);
        assert_eq!(det.len(), 1);
        assert_eq!(det[0].status, Some(Status::Perm));
        assert_eq!(det[0].at, 400);
        // Nothing new: no second detection.
        assert!(a.observe_batch(&batch[..1]).len() <= 1);
    }

    #[test]
    fn scanner_finds_sequences_with_both_terminators_amid_other_output() {
        let mut s = Osc7501Scanner::new();
        let got = bodies(
            &mut s,
            b"hello\x1b[31m\x1b]7501;state=working\x07text\x1b]0;title\x07\x1b]7501;state=done\x1b\\tail",
        );
        assert_eq!(got, ["state=working", "state=done"]);
    }

    #[test]
    fn scanner_survives_every_split_point() {
        let input = b"x\x1b]7501;state=blocked:kind=permission\x1b\\y\x1b]7501;state=idle\x07";
        for split in 0..=input.len() {
            let mut s = Osc7501Scanner::new();
            let mut got = bodies(&mut s, &input[..split]);
            got.extend(bodies(&mut s, &input[split..]));
            assert_eq!(
                got,
                ["state=blocked:kind=permission", "state=idle"],
                "split {split}"
            );
        }
    }

    #[test]
    fn scanner_drops_oversized_and_cancelled_sequences_and_recovers() {
        let mut s = Osc7501Scanner::new();
        let mut big = b"\x1b]7501;msg=".to_vec();
        big.extend(std::iter::repeat_n(b'A', MAX_SEQUENCE_BYTES + 10));
        big.extend_from_slice(b"\x07\x1b]7501;state=done\x07");
        // The oversized one is dropped; the trailing BEL of its tail is ground, the next one parses.
        assert_eq!(bodies(&mut s, &big), ["state=done"]);
        // ESC inside a body cancels it, and the ESC starts the next sequence.
        assert_eq!(
            bodies(&mut s, b"\x1b]7501;state=wor\x1b]7501;state=idle\x07"),
            ["state=idle"]
        );
        // Other OSC numbers and non-ASCII bodies are not reports.
        assert!(bodies(&mut s, b"\x1b]7500;state=done\x07\x1b]7501;state=\xff\x07").is_empty());
    }

    #[test]
    fn parses_reports_queries_and_ignores_bad_input() {
        assert_eq!(parse_body("?"), Some(Message::Query));
        assert!(is_query(" ? "));
        let Some(Message::Report(r)) = parse_body(&format!(
            "state=blocked:kind=permission:progress=40:app=codex:id=a/b:msg={}",
            b64("Run `ls`?")
        )) else {
            panic!("report expected");
        };
        assert_eq!(r.id.as_deref(), Some("a/b"));
        assert_eq!(r.state, ReportState::Blocked);
        assert_eq!(r.kind, Some(ReportKind::Permission));
        assert_eq!(r.progress, Some(40));
        assert_eq!(r.app.as_deref(), Some("codex"));
        assert_eq!(r.msg.as_deref(), Some("Run `ls`?"));

        assert_eq!(parse_body("progress=3"), None, "no state");
        assert_eq!(parse_body("state=nonsense"), None);
        assert_eq!(
            parse_body("state=working:id=bad id"),
            None,
            "bad id voids the report"
        );
        assert_eq!(parse_body("state=working:id=a//b"), None, "empty segment");
        assert_eq!(
            parse_body(&format!("state=working:msg={}", b64("a\u{7}b"))),
            None,
            "control char"
        );
        // A value that is not plain is skipped as a pair; the report still applies without it.
        let Some(Message::Report(r)) = parse_body("state=working:msg=!!!!") else {
            panic!("report expected");
        };
        assert_eq!(r.msg, None);
        assert_eq!(
            parse_body("state=working:msg=A"),
            None,
            "bad base64 length voids the report"
        );
    }

    #[test]
    fn bad_pairs_are_skipped_not_fatal() {
        let Some(Message::Report(r)) =
            parse_body("state=working:junk:Bad=1:progress=101:kind=auth")
        else {
            panic!("report expected");
        };
        assert_eq!(
            (r.state, r.kind, r.progress),
            (ReportState::Working, None, None)
        );
    }

    #[test]
    fn root_reports_map_to_statuses_and_idle_keeps_a_finished_result() {
        let mut a = Osc7501Adapter::new();
        assert_eq!(osc(&mut a, "state=idle", 1), Some((Status::Idle, 0)));
        assert_eq!(osc(&mut a, "state=working", 2), Some((Status::Run, 0)));
        assert_eq!(
            osc(&mut a, "state=working:progress=50", 3),
            None,
            "no change"
        );
        assert_eq!(osc(&mut a, "state=done", 4), Some((Status::Done, 0)));
        assert_eq!(
            osc(&mut a, "state=idle", 5),
            None,
            "idle keeps the unseen result"
        );
        assert_eq!(osc(&mut a, "state=working", 6), Some((Status::Run, 0)));
        assert_eq!(osc(&mut a, "state=error", 7), Some((Status::Fail, 0)));
        assert_eq!(osc(&mut a, "state=clear", 8), Some((Status::Unknown, 0)));
    }

    #[test]
    fn blocked_carries_the_request_and_sub_agents_roll_up() {
        let mut a = Osc7501Adapter::new();
        osc(&mut a, "state=working", 1);
        osc(&mut a, "state=working:id=w1", 2);
        let det = a.observe(&Observation::Osc(OscObservation {
            payload: format!(
                "state=blocked:kind=question:id=w1:title={}:msg={}",
                b64("Explore"),
                b64("Which db?")
            ),
            at: 3,
        }));
        let d = &det[0];
        assert_eq!(
            d.status,
            Some(Status::Perm),
            "a blocked child blocks the session"
        );
        assert_eq!(
            d.perm,
            Some(PermRequest {
                tool: "question".into(),
                summary: "Which db?".into(),
                always: None,
                id: None,
            })
        );
        let subs = d.sub_agents.as_ref().unwrap();
        assert_eq!(
            (subs[0].name.as_str(), subs[0].status),
            ("Explore", Status::Perm)
        );

        assert_eq!(
            osc(&mut a, "state=working:id=w1", 4),
            Some((Status::Run, 1))
        );
        // Lead finishes while the child still runs: the session keeps running.
        assert_eq!(osc(&mut a, "state=done", 5), None);
        assert_eq!(osc(&mut a, "state=done:id=w1", 6), Some((Status::Done, 1)));
        // Next turn drops finished children.
        assert_eq!(osc(&mut a, "state=working", 7), Some((Status::Run, 0)));
        // Removing a child by clear.
        osc(&mut a, "state=working:id=w2", 8);
        assert_eq!(osc(&mut a, "state=clear:id=w2", 9), Some((Status::Run, 0)));
    }

    #[test]
    fn process_exit_or_shell_clears_everything() {
        let mut a = Osc7501Adapter::new();
        osc(&mut a, "state=working", 1);
        let exit = Observation::Process(ProcessObservation {
            foreground: None,
            exit_code: Some(0),
            at: 2,
        });
        assert_eq!(a.observe(&exit)[0].status, Some(Status::Unknown));
        osc(&mut a, "state=working", 3);
        let shell = Observation::Process(ProcessObservation {
            foreground: Some("zsh".into()),
            exit_code: None,
            at: 4,
        });
        assert_eq!(a.observe(&shell)[0].status, Some(Status::Unknown));
        osc(&mut a, "state=working", 5);
        let agent = Observation::Process(ProcessObservation {
            foreground: Some("codex".into()),
            exit_code: None,
            at: 6,
        });
        assert!(
            a.observe(&agent).is_empty(),
            "the agent itself in the foreground changes nothing"
        );
    }

    #[test]
    fn clear_takes_its_descendants_and_a_root_clear_takes_everything() {
        let mut a = Osc7501Adapter::new();
        osc(&mut a, "state=working", 1);
        osc(&mut a, "state=working:id=a", 2);
        osc(&mut a, "state=working:id=a/b", 3);
        osc(&mut a, "state=working:id=ab", 4);
        // `a` clears `a` and `a/b`, but not the sibling `ab`.
        assert_eq!(osc(&mut a, "state=clear:id=a", 5), Some((Status::Run, 1)));
        // The root's clear leaves no running child behind to hold the session in Run.
        assert_eq!(osc(&mut a, "state=clear", 6), Some((Status::Unknown, 0)));
    }

    #[test]
    fn a_runaway_program_cannot_grow_the_record_list_without_bound() {
        let mut a = Osc7501Adapter::new();
        for i in 0..MAX_RECORDS + 50 {
            osc(&mut a, &format!("state=working:id=w{i}"), i as u64);
        }
        let det = a.derive();
        assert_eq!(det.sub_agents.len(), MAX_RECORDS);
        assert_eq!(
            det.sub_agents[0].id.as_str(),
            "w50",
            "the oldest records went first"
        );
    }

    #[test]
    fn a_report_without_title_keeps_the_sub_agent_name() {
        let mut a = Osc7501Adapter::new();
        osc(
            &mut a,
            &format!("state=working:id=w:title={}", b64("Explore")),
            1,
        );
        let det = a.observe(&Observation::Osc(OscObservation {
            payload: "state=done:id=w".into(),
            at: 2,
        }));
        assert_eq!(det[0].sub_agents.as_ref().unwrap()[0].name, "Explore");
    }

    #[test]
    fn default_adapter_skips_plain_terminals() {
        let id = SessionId::new("s");
        assert!(default_adapter(&id, AgentKind::Terminal).is_none());
        assert!(default_adapter(&id, AgentKind::Codex).is_some());
    }
}
