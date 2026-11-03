//! Glue between one session's raw signals and `lotse_detect::Engine`.
//!
//! A [`Detector`] lives inside each [`crate::session::SessionHandle`]. It turns what the daemon sees
//! into [`Observation`]s:
//!
//! | Signal | Observation |
//! |---|---|
//! | `Request::HookEvent` | `Hook` |
//! | OSC 7501 in PTY output (queries are answered with `QUERY_REPLY`) | `Osc` |
//! | PTY output → `alacritty_terminal` VT model, sampled after ~100 ms of quiet (at most every 500 ms while streaming), never inside a `?2026` synchronized update | `Screen` |
//! | foreground process group change, child exit | `Process` |
//! | ~1 s | `Tick` |
//!
//! The merged result ([`StatusUpdate`]) is applied to the session by the handle, which also decides
//! about notifications ([`notification`]).

use alacritty_terminal::event::VoidListener;
use alacritty_terminal::grid::Dimensions;
use alacritty_terminal::index::{Column, Line};
use alacritty_terminal::term::cell::Flags;
use alacritty_terminal::term::{Config as TermConfig, Term};
use alacritty_terminal::vte::ansi::Processor;
use lotse_detect::adapter::{
    HookObservation, Observation, OscObservation, ProcessObservation, ScreenObservation,
};
use lotse_detect::engine::{Engine, StatusUpdate};
use lotse_detect::osc7501::{Osc7501Scanner, is_query};
use lotse_detect::screen::{identify_agent, is_shell};
use lotse_hook::forward::{SOURCE_FIELD, SOURCE_INJECTED};
use lotse_proto::{
    AgentKind, DetectSource, Event, NotificationKind, NotificationMode, NotificationSettings,
    PermRequest, Session, SessionId, Status, UnixMs,
};
use std::collections::VecDeque;
use std::time::{Duration, Instant};

/// Output must be quiet this long before the screen is sampled.
const QUIET: Duration = Duration::from_millis(100);
/// While output keeps streaming, sample at most this often.
const STREAM_SAMPLE: Duration = Duration::from_millis(500);
/// A synchronized update that never ends is flushed after this long.
const SYNC_TIMEOUT: Duration = Duration::from_millis(150);
/// Clock ticks for timeouts inside the adapters.
const TICK: Duration = Duration::from_secs(1);
/// Most output kept for the VT model between two ticks.
const BACKLOG_MAX: usize = 256 * 1024;

struct Size {
    cols: usize,
    rows: usize,
}

impl Dimensions for Size {
    fn total_lines(&self) -> usize {
        self.rows
    }
    fn screen_lines(&self) -> usize {
        self.rows
    }
    fn columns(&self) -> usize {
        self.cols
    }
}

/// What feeding PTY output produced.
#[derive(Debug, Default)]
pub struct Fed {
    pub updates: Vec<StatusUpdate>,
    /// An OSC 7501 query was seen: write `osc7501::QUERY_REPLY` to the PTY.
    pub reply: bool,
}

/// How long an injected hook and its global twin (the same event delivered by both installs) may
/// be apart. Both are spawned by the agent at the same moment and finish within the hook deadline.
const HOOK_TWIN_WINDOW_MS: UnixMs = 1500;
/// Most hook events remembered for twin matching.
const MAX_TWIN_CANDIDATES: usize = 32;

/// A hook event waiting for its twin from the other install.
struct TwinCandidate {
    payload: serde_json::Value,
    injected: bool,
    at: UnixMs,
}

pub struct Detector {
    id: SessionId,
    /// Recent hook events not yet matched with a twin (see [`Self::is_twin`]).
    twins: VecDeque<TwinCandidate>,
    /// The agent the session was launched as; a shell session goes back to it when an agent exits.
    base_agent: AgentKind,
    engine: Engine,
    scanner: Osc7501Scanner,
    term: Term<VoidListener>,
    parser: Processor,
    /// Output not yet parsed by the VT model (see `on_output`).
    backlog: Vec<u8>,
    dirty: bool,
    last_output: Option<Instant>,
    last_sample: Option<Instant>,
    last_tick: Option<Instant>,
    /// Foreground process group seen last (`None`: not looked at yet).
    pub fg_pgid: Option<i32>,
    exited: bool,
}

impl Detector {
    pub fn new(id: SessionId, agent: AgentKind, cols: u16, rows: u16) -> Self {
        let size = Size {
            cols: usize::from(cols),
            rows: usize::from(rows),
        };
        // the raw ring keeps the scrollback; the VT model only needs the visible screen
        let config = TermConfig {
            scrolling_history: 0,
            ..TermConfig::default()
        };
        Self {
            twins: VecDeque::new(),
            engine: Engine::with_default_adapters(id.clone(), agent),
            id,
            base_agent: agent,
            scanner: Osc7501Scanner::new(),
            term: Term::new(config, &size, VoidListener),
            parser: Processor::new(),
            backlog: Vec::new(),
            dirty: false,
            last_output: None,
            last_sample: None,
            last_tick: None,
            fg_pgid: None,
            exited: false,
        }
    }

    /// The agent the engine currently follows.
    pub fn agent(&self) -> AgentKind {
        self.engine.agent()
    }

    fn set_agent(&mut self, agent: AgentKind) {
        if agent != self.engine.agent() {
            self.engine = Engine::with_default_adapters(self.id.clone(), agent);
        }
    }

    pub fn resize(&mut self, cols: u16, rows: u16) {
        self.term.resize(Size {
            cols: usize::from(cols),
            rows: usize::from(rows),
        });
    }

    fn observe(&mut self, obs: Observation) -> Option<StatusUpdate> {
        if self.exited {
            return None;
        }
        self.engine.observe(obs)
    }

    fn observe_batch(&mut self, batch: &[Observation]) -> Option<StatusUpdate> {
        if self.exited {
            return None;
        }
        self.engine.observe_batch(batch)
    }

    /// Nothing to do on a tick before the next 1 s clock tick: no unparsed output, no screen
    /// sample or synchronized update pending.
    pub fn idle(&self) -> bool {
        !self.dirty && self.backlog.is_empty() && self.parser.sync_bytes_count() == 0
    }

    pub fn on_output(&mut self, data: &[u8], now: Instant, at: UnixMs) -> Fed {
        let mut fed = Fed::default();
        if self.exited {
            return fed;
        }
        // All reports of this chunk reach the engine as one batch: one update, one broadcast.
        let mut reports = Vec::new();
        for osc in self.scanner.feed(data, at) {
            if is_query(&osc.payload) {
                fed.reply = true;
            } else {
                reports.push(Observation::Osc(OscObservation { ..osc }));
            }
        }
        if !reports.is_empty() {
            fed.updates.extend(self.observe_batch(&reports));
        }
        // The VT model is fed from `tick`, off the reader's hot path: a flood (`cat bigfile`) must
        // not slow the byte stream down. Only the newest bytes matter for the visible screen.
        self.backlog.extend_from_slice(data);
        // Trimmed in bulk (once the backlog doubles) rather than per chunk: a flood would
        // otherwise move the whole backlog on every 16 KiB read. `flush` skips the excess.
        if self.backlog.len() > 2 * BACKLOG_MAX {
            let excess = self.backlog.len() - BACKLOG_MAX;
            self.backlog.drain(..excess);
        }
        self.dirty = true;
        self.last_output = Some(now);
        fed
    }

    /// Feed the VT model with what arrived since the last call.
    fn flush(&mut self) {
        if !self.backlog.is_empty() {
            let data = std::mem::take(&mut self.backlog);
            let newest = &data[data.len().saturating_sub(BACKLOG_MAX)..];
            self.parser.advance(&mut self.term, newest);
        }
    }

    /// `true` when `payload` is the second copy of an event already seen from the other install.
    ///
    /// With the global hooks installed *and* the plugin Lotse injects, an agent runs both for every
    /// event. `lotse-hook --injected` tags its copy (`lotse_source`). An event equal to a waiting
    /// one from the *other* source, seen within [`HOOK_TWIN_WINDOW_MS`], is that event's copy and is
    /// dropped. Events from one source never match each other, so a legitimate repeat (two
    /// identical `Stop`s in a row) is kept. The tag is removed from `payload` either way.
    fn is_twin(&mut self, payload: &mut serde_json::Value, at: UnixMs) -> bool {
        let injected = payload
            .as_object_mut()
            .and_then(|o| o.remove(SOURCE_FIELD))
            .is_some_and(|v| v == SOURCE_INJECTED);
        self.twins
            .retain(|t| at.saturating_sub(t.at) <= HOOK_TWIN_WINDOW_MS);
        if let Some(i) = self
            .twins
            .iter()
            .position(|t| t.injected != injected && t.payload == *payload)
        {
            self.twins.remove(i);
            return true;
        }
        if self.twins.len() == MAX_TWIN_CANDIDATES {
            self.twins.pop_front();
        }
        self.twins.push_back(TwinCandidate {
            payload: payload.clone(),
            injected,
            at,
        });
        false
    }

    pub fn on_hook(
        &mut self,
        agent: AgentKind,
        mut payload: serde_json::Value,
        at: UnixMs,
    ) -> Option<StatusUpdate> {
        if self.is_twin(&mut payload, at) {
            return None;
        }
        if !self.exited {
            self.set_agent(agent);
        }
        self.observe(Observation::Hook(HookObservation { agent, payload, at }))
    }

    /// The foreground process group changed; `name` is its executable.
    pub fn on_foreground(&mut self, name: String, at: UnixMs) -> Vec<StatusUpdate> {
        let mut out = Vec::new();
        if let Some(agent) = identify_agent(&name) {
            self.set_agent(agent);
        } else if is_shell(&name) && self.engine.agent() != self.base_agent {
            // the agent quit back into the shell: process identity ends its signals
            out.extend(self.observe(Observation::Process(ProcessObservation {
                foreground: Some(name),
                exit_code: None,
                at,
            })));
            self.engine = Engine::with_default_adapters(self.id.clone(), self.base_agent);
            out.push(StatusUpdate {
                status: if self.base_agent == AgentKind::Terminal {
                    Status::Idle
                } else {
                    Status::Unknown
                },
                source: DetectSource::Process,
                perm: None,
                sub_agents: Some(Vec::new()),
                running_since: None,
                at,
            });
            return out;
        }
        out.extend(self.observe(Observation::Process(ProcessObservation {
            foreground: Some(name),
            exit_code: None,
            at,
        })));
        out
    }

    /// The child exited. The handle sets the final status itself; this resets the adapters.
    pub fn on_exit(&mut self, code: Option<i32>, at: UnixMs) {
        self.observe(Observation::Process(ProcessObservation {
            foreground: None,
            exit_code: code,
            at,
        }));
        self.exited = true;
    }

    /// Restart after a retry: fresh engine and screen.
    pub fn reset(&mut self, cols: u16, rows: u16) {
        *self = Self::new(self.id.clone(), self.base_agent, cols, rows);
    }

    /// Called about every 100 ms: screen sample, synchronized-update timeout, 1 s tick.
    pub fn tick(&mut self, now: Instant, at: UnixMs) -> Vec<StatusUpdate> {
        let mut out = Vec::new();
        if self.exited {
            return out;
        }
        self.flush();
        let quiet_for = self.last_output.map(|t| now.saturating_duration_since(t));
        if self.parser.sync_bytes_count() > 0 && quiet_for.is_some_and(|q| q >= SYNC_TIMEOUT) {
            // an update that never ended: show what we have, like a terminal would
            self.parser.stop_sync(&mut self.term);
        }
        let in_sync = self.parser.sync_bytes_count() > 0;
        let due = quiet_for.is_some_and(|q| q >= QUIET)
            || self
                .last_sample
                .is_none_or(|s| now.saturating_duration_since(s) >= STREAM_SAMPLE);
        if self.dirty && !in_sync && due {
            self.dirty = false;
            self.last_sample = Some(now);
            let rows = self.screen_rows();
            out.extend(self.observe(Observation::Screen(ScreenObservation { rows, at })));
        }
        if self
            .last_tick
            .is_none_or(|t| now.saturating_duration_since(t) >= TICK)
        {
            self.last_tick = Some(now);
            out.extend(self.observe(Observation::Tick { now: at }));
        }
        out
    }

    /// Visible rows, top to bottom, trailing blanks of each row trimmed.
    pub fn screen_rows(&mut self) -> Vec<String> {
        self.flush();
        let grid = self.term.grid();
        let mut text = String::new(); // one scratch buffer for every row
        (0..grid.screen_lines())
            .map(|l| {
                let line = &grid[Line(l as i32)];
                text.clear();
                text.extend(
                    (0..grid.columns())
                        .map(|c| &line[Column(c)])
                        .filter(|cell| !cell.flags.contains(Flags::WIDE_CHAR_SPACER))
                        .map(|cell| cell.c),
                );
                text.trim_end().to_owned()
            })
            .collect()
    }
}

/// Executable name of a process (`ps -o comm=`), for process identity. macOS and Linux `ps` agree.
pub fn process_name(pid: i32) -> Option<String> {
    let out = std::process::Command::new("/bin/ps")
        .args(["-o", "comm=", "-p", &pid.to_string()])
        .stdin(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .output()
        .ok()?;
    let name = String::from_utf8_lossy(&out.stdout).trim().to_owned();
    (!name.is_empty()).then_some(name)
}

// ---- notifications ------------------------------------------------------------------------------

/// Why a notification may be due.
#[derive(Debug, Clone, PartialEq)]
pub enum Trigger {
    Perm(Option<PermRequest>),
    Fail,
    Done,
    SubAgentDone(String),
}

impl Trigger {
    fn kind(&self) -> NotificationKind {
        match self {
            Trigger::Perm(_) => NotificationKind::Perm,
            Trigger::Fail => NotificationKind::Fail,
            Trigger::Done => NotificationKind::Done,
            Trigger::SubAgentDone(_) => NotificationKind::SubAgentDone,
        }
    }
}

/// Window in which a session gets at most one notification of each kind.
pub const NOTIFY_GAP: Duration = Duration::from_secs(2);
/// A permission request that differs from the last notified one is a new reason to look up (the
/// contract of `Trigger::Perm`), so it may pass inside the window, this many times at most.
const MAX_DISTINCT_PERMS: u32 = 5;

#[derive(Debug, Default)]
struct Slot {
    window_start: Option<Instant>,
    sent: u32,
    last_perm: Option<PermRequest>,
}

/// At most one Perm / Done / Fail notification per kind per [`NOTIFY_GAP`] (Perm: also each new
/// request, up to [`MAX_DISTINCT_PERMS`]): a program that toggles its status in a loop must not
/// flood the user. The status itself still updates every time.
#[derive(Debug, Default)]
pub struct NotifyLimiter {
    slots: [Slot; 3],
}

impl NotifyLimiter {
    pub fn allow(&mut self, trigger: &Trigger, now: Instant) -> bool {
        let (slot, perm) = match trigger {
            Trigger::Perm(p) => (&mut self.slots[0], p.as_ref()),
            Trigger::Fail => (&mut self.slots[1], None),
            Trigger::Done => (&mut self.slots[2], None),
            Trigger::SubAgentDone(_) => return true,
        };
        if slot
            .window_start
            .is_none_or(|t| now.saturating_duration_since(t) >= NOTIFY_GAP)
        {
            slot.window_start = Some(now);
            slot.sent = 0;
        }
        // the same request is not new when only its prompt id changed (Perm, Run, Perm again)
        let same = |a: &PermRequest, b: &PermRequest| a.tool == b.tool && a.summary == b.summary;
        let new_request = perm.is_some_and(|p| slot.last_perm.as_ref().is_none_or(|l| !same(p, l)));
        let allowed = slot.sent == 0 || (new_request && slot.sent < MAX_DISTINCT_PERMS);
        if allowed {
            slot.sent += 1;
            slot.last_perm = perm.cloned();
        }
        allowed
    }
}

/// What a notification shows and plays.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Resolved {
    pub banner: bool,
    pub sound: bool,
    /// The global row's macOS system sound; only with `sound`.
    pub sound_name: Option<String>,
}

/// Settings + per-project override → what to show and play, `None` when there is nothing to do
/// (mock `notifies`). A custom "off" and Mute silence the whole event, sound included; a custom "on"
/// takes the sound from the global row. Banner off with sound on is a sound-only event.
pub fn resolve(
    settings: &NotificationSettings,
    mode: &NotificationMode,
    kind: NotificationKind,
) -> Option<Resolved> {
    let global = match kind {
        NotificationKind::Perm => &settings.perm,
        NotificationKind::Fail => &settings.fail,
        NotificationKind::Done => &settings.done,
        NotificationKind::SubAgentDone => &settings.sub_agent_done,
    };
    let banner = match mode {
        NotificationMode::Mute => return None,
        NotificationMode::Global => global.banner,
        NotificationMode::Custom {
            perm,
            fail,
            done,
            sub_agent_done,
        } => match kind {
            NotificationKind::Perm => *perm,
            NotificationKind::Fail => *fail,
            NotificationKind::Done => *done,
            NotificationKind::SubAgentDone => *sub_agent_done,
        },
    };
    let custom_off = matches!(mode, NotificationMode::Custom { .. }) && !banner;
    if custom_off || !(banner || global.sound) {
        return None;
    }
    Some(Resolved {
        banner,
        sound: global.sound,
        sound_name: global
            .sound_name
            .clone()
            .filter(|n| global.sound && !n.is_empty()),
    })
}

/// The `Event::Notification` for a trigger, or `None` when settings or the project say no.
/// Pause and focus are the app's call (it knows the window).
pub fn notification(
    settings: &NotificationSettings,
    mode: &NotificationMode,
    project_name: &str,
    session: &Session,
    trigger: &Trigger,
    at: UnixMs,
) -> Option<Event> {
    let kind = trigger.kind();
    let shown = resolve(settings, mode, kind)?;
    let prompt_id = match trigger {
        Trigger::Perm(Some(p)) => p.id.clone(),
        _ => None,
    };
    let (subtitle, body) = match trigger {
        Trigger::Perm(Some(p)) => (
            "Needs permission",
            Some(format!("{}({})", p.tool, p.summary)),
        ),
        Trigger::Perm(None) => ("Needs permission", None),
        Trigger::Fail => ("Failed", Some(session.title.clone())),
        Trigger::Done => ("Finished", Some(session.title.clone())),
        Trigger::SubAgentDone(name) => ("Sub-agent finished", Some(name.clone())),
    };
    Some(Event::Notification {
        session_id: session.id.clone(),
        kind,
        title: format!("{project_name} · {}", session.agent.label()),
        subtitle: subtitle.into(),
        body,
        banner: shown.banner,
        sound: shown.sound,
        sound_name: shown.sound_name,
        prompt_id,
        at,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use lotse_proto::EventPrefs;

    fn detector(agent: AgentKind) -> Detector {
        Detector::new(SessionId::new("s-1"), agent, 80, 24)
    }

    fn feed(d: &mut Detector, text: &str, t0: Instant, ms: u64) {
        d.on_output(text.as_bytes(), t0 + Duration::from_millis(ms), 1_000 + ms);
    }

    #[test]
    fn the_vt_model_renders_the_visible_screen() {
        let mut d = detector(AgentKind::Terminal);
        let t0 = Instant::now();
        feed(&mut d, "hello\r\n\x1b[1mworld\x1b[0m  ", t0, 0);
        let rows = d.screen_rows();
        assert_eq!(rows.len(), 24);
        assert_eq!(rows[0], "hello");
        assert_eq!(rows[1], "world");
        assert_eq!(rows[2], "");
    }

    #[test]
    fn a_chunk_of_osc_toggles_is_one_update_and_notifies_at_most_once_per_kind() {
        let mut d = detector(AgentKind::ClaudeCode);
        let mut chunk = String::new();
        for n in 0..400 {
            let state = ["working", "blocked", "done", "error"][n % 4];
            chunk.push_str(&format!("\x1b]7501;state={state}\x07"));
        }
        chunk.push_str("\x1b]7501;state=blocked\x07");
        let fed = d.on_output(chunk.as_bytes(), Instant::now(), 5);
        assert_eq!(fed.updates.len(), 1);
        assert_eq!(fed.updates[0].status, Status::Perm);

        // the same flood arriving as many chunks: every status update goes through, but the
        // notifications are limited to one per kind per gap
        let t0 = Instant::now();
        let mut limiter = NotifyLimiter::default();
        let triggers = [Trigger::Perm(None), Trigger::Fail, Trigger::Done];
        let allowed = (0..300u64)
            .filter(|n| {
                limiter.allow(
                    &triggers[(*n % 3) as usize],
                    t0 + Duration::from_millis(n * 5),
                )
            })
            .count();
        assert_eq!(allowed, 3);
        assert!(limiter.allow(&Trigger::Fail, t0 + NOTIFY_GAP + Duration::from_millis(10)));
        assert!(limiter.allow(&Trigger::SubAgentDone("a".into()), t0));
        assert!(limiter.allow(&Trigger::SubAgentDone("a".into()), t0));

        // distinct permission requests are new reasons to look up, but only a few per window
        let mut limiter = NotifyLimiter::default();
        let allowed = (0..100)
            .filter(|n| {
                let req = PermRequest {
                    tool: "Bash".into(),
                    summary: format!("cmd {n}"),
                    always: None,
                    id: None,
                };
                limiter.allow(&Trigger::Perm(Some(req)), t0)
            })
            .count();
        assert_eq!(allowed, MAX_DISTINCT_PERMS as usize);
    }

    /// Perm(X), Run, Perm(X) hands out a new prompt id each time; it is still one request.
    #[test]
    fn the_same_request_with_a_new_prompt_id_gets_one_banner_per_window() {
        let t0 = Instant::now();
        let ask = |id: &str, summary: &str| {
            Trigger::Perm(Some(PermRequest {
                tool: "Bash".into(),
                summary: summary.into(),
                always: None,
                id: Some(id.into()),
            }))
        };
        let mut limiter = NotifyLimiter::default();
        let allowed = (0..50)
            .filter(|n| limiter.allow(&ask(&format!("id-{n}"), "ls"), t0))
            .count();
        assert_eq!(allowed, 1, "a flapping request is one banner");
        // a different request still passes inside the window
        assert!(limiter.allow(&ask("id-x", "rm -rf build"), t0));
    }

    #[test]
    fn the_backlog_trims_in_bulk_but_the_vt_model_sees_only_the_newest_bytes() {
        let mut d = detector(AgentKind::Terminal);
        let t0 = Instant::now();
        let filler = "x".repeat(16 * 1024);
        for n in 0..80 {
            feed(&mut d, &filler, t0, n);
        }
        assert!(d.backlog.len() <= 2 * BACKLOG_MAX);
        feed(&mut d, "\r\nlast line", t0, 90);
        let rows = d.screen_rows();
        assert!(rows.iter().any(|r| r == "last line"), "{rows:?}");
    }

    #[test]
    fn osc_queries_are_answered_not_observed() {
        let mut d = detector(AgentKind::Terminal);
        // split across two chunks: the scanner keeps state between them
        let first = d.on_output(b"\x1b]7501;", Instant::now(), 1);
        assert!(!first.reply);
        let fed = d.on_output(b"?\x1b\\", Instant::now(), 2);
        assert!(fed.reply);
        assert!(fed.updates.is_empty());
    }

    #[test]
    fn screen_is_sampled_after_quiet_and_not_inside_a_synchronized_update() {
        let mut d = detector(AgentKind::Codex);
        let t0 = Instant::now();
        // begin a synchronized update, never end it
        feed(&mut d, "\x1b[?2026hpartial frame", t0, 0);
        // quiet for 120 ms but still inside the update: no sample (dirty stays)
        d.tick(t0 + Duration::from_millis(120), 1_120);
        assert!(d.parser.sync_bytes_count() > 0);
        assert!(d.dirty, "no sampling inside ?2026");
        // past the sync timeout the update is flushed and the screen sampled
        d.tick(t0 + Duration::from_millis(300), 1_300);
        assert!(!d.dirty);
        assert!(d.screen_rows()[0].contains("partial frame"));
    }

    #[test]
    fn streaming_output_is_sampled_at_most_every_500_ms() {
        let mut d = detector(AgentKind::Codex);
        let t0 = Instant::now();
        feed(&mut d, "a", t0, 0);
        d.tick(t0 + Duration::from_millis(10), 1_010); // first sample (no earlier one)
        assert!(!d.dirty);
        feed(&mut d, "b", t0, 20);
        d.tick(t0 + Duration::from_millis(30), 1_030); // streaming, last sample 20 ms ago
        assert!(d.dirty, "too soon, still streaming");
        d.tick(t0 + Duration::from_millis(600), 1_600);
        assert!(!d.dirty);
    }

    #[test]
    fn hook_event_switches_the_engine_to_the_hook_agent_and_reports_status() {
        let mut d = detector(AgentKind::Terminal);
        let up = d
            .on_hook(
                AgentKind::ClaudeCode,
                serde_json::json!({"hook_event_name": "UserPromptSubmit"}),
                5,
            )
            .expect("claude starts running");
        assert_eq!(up.status, Status::Run);
        assert_eq!(d.agent(), AgentKind::ClaudeCode);
    }

    fn hook(d: &mut Detector, name: &str, source: Option<&str>, at: UnixMs) -> Option<Status> {
        let mut payload = serde_json::json!({"hook_event_name": name});
        if let Some(source) = source {
            payload["lotse_source"] = source.into();
        }
        d.on_hook(AgentKind::ClaudeCode, payload, at)
            .map(|u| u.status)
    }

    #[test]
    fn the_twin_of_an_injected_hook_from_the_global_install_is_dropped() {
        let mut d = detector(AgentKind::ClaudeCode);
        assert_eq!(
            hook(&mut d, "UserPromptSubmit", Some("injected"), 100),
            Some(Status::Run)
        );
        assert_eq!(
            hook(&mut d, "Stop", Some("injected"), 200),
            Some(Status::Done)
        );
        // the global hook's copy of the prompt event arrives late: it must not restart the turn
        assert_eq!(hook(&mut d, "UserPromptSubmit", None, 250), None);
        assert_eq!(d.engine.status(), Status::Done);
        // its copy of Stop is the twin of the second event and is dropped as well
        assert_eq!(hook(&mut d, "Stop", None, 300), None);
    }

    #[test]
    fn codex_hooks_start_in_a_shell_pane_and_the_twin_of_an_injected_one_is_dropped() {
        let codex = |d: &mut Detector, name: &str, source: Option<&str>, at: UnixMs| {
            let mut payload = serde_json::json!({"hook_event_name": name});
            if let Some(source) = source {
                payload["lotse_source"] = source.into();
            }
            d.on_hook(AgentKind::Codex, payload, at).map(|u| u.status)
        };
        // `codex` started by hand in a plain terminal pane: its first hook makes the pane a Codex one
        let mut d = detector(AgentKind::Terminal);
        assert_eq!(
            codex(&mut d, "UserPromptSubmit", Some("injected"), 100),
            Some(Status::Run)
        );
        assert_eq!(d.agent(), AgentKind::Codex);
        assert_eq!(
            codex(&mut d, "Interrupt", Some("injected"), 200),
            Some(Status::Done)
        );
        // a second hook install delivering the same events is deduplicated
        assert_eq!(codex(&mut d, "UserPromptSubmit", None, 250), None);
        assert_eq!(codex(&mut d, "Interrupt", None, 300), None);
        assert_eq!(d.engine.status(), Status::Done);
    }

    #[test]
    fn repeats_from_one_source_and_old_twins_are_real_events() {
        let mut d = detector(AgentKind::ClaudeCode);
        assert_eq!(
            hook(&mut d, "UserPromptSubmit", Some("injected"), 100),
            Some(Status::Run)
        );
        assert_eq!(
            hook(&mut d, "Stop", Some("injected"), 200),
            Some(Status::Done)
        );
        assert_eq!(
            hook(&mut d, "UserPromptSubmit", Some("injected"), 300),
            Some(Status::Run)
        );
        // same payload from the other source, but long after the first: a new turn, not a twin
        assert_eq!(
            hook(&mut d, "Stop", None, 300 + HOOK_TWIN_WINDOW_MS + 1),
            Some(Status::Done)
        );
        // an unmatched old event is forgotten
        assert_eq!(
            hook(&mut d, "UserPromptSubmit", None, 10_000),
            Some(Status::Run)
        );
    }

    #[test]
    fn agent_quitting_back_to_the_shell_resets_a_shell_session_to_idle() {
        let mut d = detector(AgentKind::Terminal);
        d.on_hook(
            AgentKind::ClaudeCode,
            serde_json::json!({"hook_event_name": "UserPromptSubmit"}),
            5,
        );
        let ups = d.on_foreground("/bin/zsh".into(), 9);
        assert_eq!(d.agent(), AgentKind::Terminal);
        let last = ups.last().expect("reset update");
        assert_eq!(last.status, Status::Idle);
        assert_eq!(last.sub_agents, Some(Vec::new()));
    }

    #[test]
    fn identify_agent_in_the_foreground_switches_the_engine() {
        let mut d = detector(AgentKind::Terminal);
        d.on_foreground("/opt/homebrew/bin/codex".into(), 1);
        assert_eq!(d.agent(), AgentKind::Codex);
    }

    fn settings() -> NotificationSettings {
        NotificationSettings::default()
    }

    fn session(agent: AgentKind) -> Session {
        serde_json::from_value(serde_json::json!({
            "id": "s-1", "project_id": "p-1", "title": "Fix login", "agent": agent,
            "command": "claude", "status": "perm", "sub_agents": [],
            "git": {"is_git": true, "branch": "main", "ahead": 0, "dirty": 0, "worktree": false, "worktree_path": null},
            "created_at": 1, "last_activity": 1, "running_since": null, "unread": false,
            "perm": null, "source": "hook"
        }))
        .unwrap()
    }

    #[test]
    fn notifications_follow_the_mock_defaults() {
        let s = session(AgentKind::ClaudeCode);
        let perm = Trigger::Perm(Some(PermRequest {
            tool: "Bash".into(),
            summary: "npx playwright test e2e/patients".into(),
            always: None,
            id: Some("p-1".into()),
        }));
        let Some(Event::Notification {
            title,
            subtitle,
            body,
            sound,
            ..
        }) = notification(
            &settings(),
            &NotificationMode::Global,
            "praxis-web",
            &s,
            &perm,
            1,
        )
        else {
            panic!("perm banner is on by default");
        };
        assert_eq!(title, "praxis-web · Claude Code");
        assert_eq!(subtitle, "Needs permission");
        assert_eq!(
            body.as_deref(),
            Some("Bash(npx playwright test e2e/patients)")
        );
        assert!(sound);
        // the banner's Allow/Deny answers this prompt only
        let again = notification(&settings(), &NotificationMode::Global, "p", &s, &perm, 1);
        assert!(matches!(
            again,
            Some(Event::Notification { prompt_id: Some(ref id), banner: true, .. }) if id == "p-1"
        ));
        // done: banner, no sound; sub-agent done: off
        let done = notification(
            &settings(),
            &NotificationMode::Global,
            "p",
            &s,
            &Trigger::Done,
            1,
        );
        assert!(matches!(
            done,
            Some(Event::Notification { sound: false, .. })
        ));
        let sub = Trigger::SubAgentDone("explore-repo".into());
        assert!(notification(&settings(), &NotificationMode::Global, "p", &s, &sub, 1).is_none());
    }

    fn prefs(banner: bool, sound: bool, sound_name: Option<&str>) -> EventPrefs {
        EventPrefs {
            banner,
            sound,
            allow_deny: false,
            sound_name: sound_name.map(Into::into),
        }
    }

    /// `(banner, sound, sound_name)` of the notification a `done` trigger gets, `None` for no event.
    fn done_with(
        settings: &NotificationSettings,
        mode: &NotificationMode,
    ) -> Option<(bool, bool, Option<String>)> {
        let s = session(AgentKind::ClaudeCode);
        match notification(settings, mode, "p", &s, &Trigger::Done, 1)? {
            Event::Notification {
                banner,
                sound,
                sound_name,
                ..
            } => Some((banner, sound, sound_name)),
            other => panic!("not a notification: {other:?}"),
        }
    }

    #[test]
    fn sound_without_banner_is_a_sound_only_event() {
        let sound_only = NotificationSettings {
            done: prefs(false, true, None),
            ..settings()
        };
        assert_eq!(
            done_with(&sound_only, &NotificationMode::Global),
            Some((false, true, None))
        );
        // both off: nothing to show or play
        let off = NotificationSettings {
            done: prefs(false, false, Some("Glass")),
            ..settings()
        };
        assert_eq!(done_with(&off, &NotificationMode::Global), None);
    }

    #[test]
    fn the_sound_name_comes_from_the_global_row_and_needs_the_sound() {
        let named = NotificationSettings {
            done: prefs(true, true, Some("Glass")),
            ..settings()
        };
        assert_eq!(
            done_with(&named, &NotificationMode::Global),
            Some((true, true, Some("Glass".into())))
        );
        // a name on a silent row is not sent
        let silent = NotificationSettings {
            done: prefs(true, false, Some("Glass")),
            ..settings()
        };
        assert_eq!(
            done_with(&silent, &NotificationMode::Global),
            Some((true, false, None))
        );
        // an empty name is no name
        let blank = NotificationSettings {
            done: prefs(true, true, Some("")),
            ..settings()
        };
        assert_eq!(
            done_with(&blank, &NotificationMode::Global),
            Some((true, true, None))
        );
        // a custom "on" keeps the global sound and name
        let custom = NotificationMode::Custom {
            perm: false,
            fail: false,
            done: true,
            sub_agent_done: false,
        };
        assert_eq!(
            done_with(&named, &custom),
            Some((true, true, Some("Glass".into())))
        );
    }

    #[test]
    fn mute_and_a_custom_off_silence_sound_only_rows_too() {
        let sound_only = NotificationSettings {
            done: prefs(false, true, Some("Glass")),
            ..settings()
        };
        assert_eq!(done_with(&sound_only, &NotificationMode::Mute), None);
        let custom_off = NotificationMode::Custom {
            perm: true,
            fail: true,
            done: false,
            sub_agent_done: true,
        };
        assert_eq!(done_with(&sound_only, &custom_off), None);
    }

    #[test]
    fn mute_and_custom_overrides() {
        let s = session(AgentKind::Codex);
        assert!(
            notification(
                &settings(),
                &NotificationMode::Mute,
                "p",
                &s,
                &Trigger::Fail,
                1
            )
            .is_none()
        );
        let custom = NotificationMode::Custom {
            perm: false,
            fail: false,
            done: false,
            sub_agent_done: true,
        };
        let sub = Trigger::SubAgentDone("x".into());
        assert!(notification(&settings(), &custom, "p", &s, &sub, 1).is_some());
        assert!(notification(&settings(), &custom, "p", &s, &Trigger::Fail, 1).is_none());
        // WP09: banner off + sound on is now a sound-only event instead of nothing
        let sound_only = NotificationSettings {
            fail: prefs(false, true, None),
            ..settings()
        };
        assert!(matches!(
            notification(
                &sound_only,
                &NotificationMode::Global,
                "p",
                &s,
                &Trigger::Fail,
                1
            ),
            Some(Event::Notification {
                banner: false,
                sound: true,
                ..
            })
        ));
    }
}
