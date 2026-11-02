//! OpenCode adapter: maps the events of the Lotse OpenCode plugin to status.
//! Source: [`DetectSource::Hook`] (the plugin is OpenCode's hook mechanism).
//!
//! # Where the events come from
//!
//! `integrations/opencode/lotse-status.js` runs inside OpenCode, listens to OpenCode's bus events and
//! pipes one JSON object per event into `lotse-hook opencode`, which forwards it as
//! `Request::HookEvent { agent: OpenCode, payload }` (see that file for how). The payload is Lotse's
//! own small contract, not OpenCode's raw event, so the adapter never sees conversation text:
//!
//! | `hook_event_name` | Fields (besides `session_id`, and `parent_id` when the session is a child) | OpenCode event it comes from |
//! |---|---|---|
//! | `SessionCreated` | `title?` | `session.created` (`properties.info.{id,parentID,title}`) |
//! | `SessionStatus` | `status`: `busy`, `retry` or `idle` | `session.status` (`properties.status.type`) |
//! | `SessionIdle` | | `session.idle` (deprecated twin of `status: idle`) |
//! | `SessionError` | `message?` | `session.error` |
//! | `SessionDeleted` | | `session.deleted` |
//! | `PermissionAsked` | `request_id`, `tool?`, `summary?` | `permission.asked` |
//! | `PermissionReplied` | `request_id?` | `permission.replied` |
//! | `QuestionAsked` | `request_id`, `summary?` | `question.asked` |
//! | `QuestionReplied`, `QuestionRejected` | `request_id?` | `question.replied`, `question.rejected` |
//!
//! The OpenCode event names and the `sessionID`, `info.parentID`, `status.type` and request `id`
//! fields are taken from Orca's OpenCode status plugin (`src/main/opencode/status-plugin-*-source.ts`
//! in `/Users/naim/Repos/research/orca`, read 2026-11-02). **Unverified:** the OpenCode plugin docs
//! (<https://opencode.ai/docs/plugins/>) and event reference could not be read (no web access), and the
//! payload fields of `permission.asked` that feed `tool` / `summary` (`permission`, `patterns`) are
//! from memory of the SDK types; check both before release. Unknown events and missing fields never
//! change the status.
//!
//! # Status table
//!
//! Each OpenCode session is tracked on its own. A session with a `parent_id` is a sub-agent (OpenCode
//! creates child sessions for the task tool); the others are *roots*. A session's state: `Perm` while
//! it has an unanswered permission or question, else `Run` while busy, else `Fail` if a
//! `SessionError` arrived since it last started, else `Done` after at least one turn, else `Idle`.
//!
//! | Event | Effect |
//! |---|---|
//! | `SessionStatus` `busy` / `retry` | session busy, error flag cleared; a root that starts a new turn drops the finished sub-agents of the last one |
//! | `SessionStatus` `idle`, `SessionIdle` | session no longer busy; its open permission and question requests are dropped (a turn ended) |
//! | `SessionError` | error flag set: the session shows `Fail` once it is idle (recoverable errors are followed by another `busy`). The plugin does not send it for a user abort (`MessageAbortedError`) |
//! | `PermissionAsked`, `QuestionAsked` | session `Perm` with `PermRequest { tool, summary }` |
//! | `PermissionReplied`, `QuestionReplied`, `QuestionRejected` | that request is closed |
//! | `SessionDeleted` | session removed |
//! | `Observation::Tick` | a busy session silent for 30 min is treated as idle (its events were lost); the plugin repeats `busy` every 5 min while a session is busy, so a long quiet turn is not written off |
//! | PTY child exited, or a shell took the foreground | everything cleared (`Unknown`) |
//!
//! The reported status is derived: any `Perm` session (root or sub-agent) makes it `Perm`; else `Run`
//! when any root is busy or any sub-agent is busy (so a finished lead does not end the card while
//! children work); else the most recently active root's resting state.

use crate::adapter::{Adapter, Detection, HookObservation, Observation};
use lotse_proto::{
    AgentKind, DetectSource, PermRequest, SessionId, Status, SubAgent, SubAgentId, UnixMs,
};
use serde_json::Value;

/// Plugin docs (**Unverified** against the live page; see the module docs).
pub const PLUGIN_DOC_URL: &str = "https://opencode.ai/docs/plugins/";

/// Longest `PermRequest::summary`, in characters.
const SUMMARY_MAX_CHARS: usize = 160;
/// Plugin payloads come from any local process: strings kept in memory are clipped and the
/// per-session records (sessions, pending requests) are bounded, oldest dropped first.
const TOOL_MAX_CHARS: usize = 40;
const ID_MAX_CHARS: usize = 128;
const MAX_RECORDS: usize = 256;
/// A busy session that has been silent this long lost its idle event.
const BUSY_EXPIRE_MS: UnixMs = 30 * 60 * 1000;

/// Called by `Engine::with_default_adapters`: the plugin adapter for `AgentKind::OpenCode`.
pub fn default_adapter(_session: &SessionId, agent: AgentKind) -> Option<Box<dyn Adapter>> {
    (agent == AgentKind::OpenCode).then(|| Box::new(OpenCodeAdapter::new()) as Box<dyn Adapter>)
}

#[derive(Debug, Clone, PartialEq)]
struct Request {
    id: String,
    req: PermRequest,
}

#[derive(Debug, Clone, PartialEq)]
struct Session {
    id: String,
    parent: Option<String>,
    name: String,
    busy: bool,
    failed: bool,
    turn_seen: bool,
    pending: Vec<Request>,
    last_at: UnixMs,
}

impl Session {
    fn status(&self) -> Status {
        if !self.pending.is_empty() {
            Status::Perm
        } else if self.busy {
            Status::Run
        } else if self.failed {
            Status::Fail
        } else if self.turn_seen {
            Status::Done
        } else {
            Status::Idle
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
struct Reported {
    status: Status,
    perm: Option<PermRequest>,
    sub_agents: Vec<SubAgent>,
}

/// Plugin-event adapter for one OpenCode session in a Lotse pane.
#[derive(Debug, Default)]
pub struct OpenCodeAdapter {
    sessions: Vec<Session>,
    reported: Option<Reported>,
}

impl OpenCodeAdapter {
    pub fn new() -> Self {
        Self::default()
    }

    fn index(&mut self, id: &str, parent: Option<&str>, at: UnixMs) -> usize {
        let at_index = match self.sessions.iter().position(|s| s.id == id) {
            Some(i) => i,
            None => {
                if self.sessions.len() >= MAX_RECORDS {
                    self.sessions.remove(0);
                }
                self.sessions.push(Session {
                    id: id.to_owned(),
                    parent: None,
                    name: String::new(),
                    busy: false,
                    failed: false,
                    turn_seen: false,
                    pending: Vec::new(),
                    last_at: at,
                });
                self.sessions.len() - 1
            }
        };
        let session = &mut self.sessions[at_index];
        if session.parent.is_none() {
            session.parent = parent.map(str::to_owned);
        }
        session.last_at = at;
        at_index
    }

    /// Apply one plugin payload. `false`: not an event this adapter acts on.
    fn handle_hook(&mut self, payload: &Value, at: UnixMs) -> bool {
        let (Some(event), Some(id)) = (
            str_field(payload, "hook_event_name"),
            id_field(payload, "session_id"),
        ) else {
            return false;
        };
        let parent = id_field(payload, "parent_id");
        match event {
            "SessionCreated" => {
                let i = self.index(id, parent, at);
                if let Some(title) = str_field(payload, "title").filter(|t| !t.is_empty()) {
                    self.sessions[i].name = clip(title, 80);
                }
            }
            "SessionStatus" => match str_field(payload, "status") {
                Some("busy" | "retry") => self.start_turn(id, parent, at),
                Some("idle") => self.end_turn(id, parent, at),
                _ => return false,
            },
            "SessionIdle" => self.end_turn(id, parent, at),
            "SessionError" => {
                let i = self.index(id, parent, at);
                self.sessions[i].failed = true;
            }
            "SessionDeleted" => self
                .sessions
                .retain(|s| s.id != id && s.parent.as_deref() != Some(id)),
            "PermissionAsked" | "QuestionAsked" => {
                let question = event == "QuestionAsked";
                let i = self.index(id, parent, at);
                let tool = if question {
                    "question"
                } else {
                    str_field(payload, "tool")
                        .filter(|t| !t.is_empty())
                        .unwrap_or("permission")
                };
                let request = Request {
                    id: id_field(payload, "request_id")
                        .unwrap_or_default()
                        .to_owned(),
                    req: PermRequest {
                        tool: clip(tool, TOOL_MAX_CHARS),
                        summary: clip(
                            str_field(payload, "summary").unwrap_or_default(),
                            SUMMARY_MAX_CHARS,
                        ),
                        always: None,
                    },
                };
                let pending = &mut self.sessions[i].pending;
                match pending.iter_mut().find(|r| r.id == request.id) {
                    Some(slot) => *slot = request,
                    None => {
                        if pending.len() >= MAX_RECORDS {
                            pending.remove(0);
                        }
                        pending.push(request);
                    }
                }
            }
            "PermissionReplied" | "QuestionReplied" | "QuestionRejected" => {
                let i = self.index(id, parent, at);
                match id_field(payload, "request_id").filter(|r| !r.is_empty()) {
                    Some(request) => self.sessions[i].pending.retain(|r| r.id != request),
                    // No id to match: the user answered whatever was open.
                    None => self.sessions[i].pending.clear(),
                }
            }
            _ => return false,
        }
        true
    }

    fn start_turn(&mut self, id: &str, parent: Option<&str>, at: UnixMs) {
        let i = self.index(id, parent, at);
        let was_idle = !self.sessions[i].busy;
        let is_root = self.sessions[i].parent.is_none();
        let s = &mut self.sessions[i];
        s.busy = true;
        s.failed = false;
        s.turn_seen = true;
        if is_root && was_idle {
            // A new turn: sub-agents that finished in the previous one go.
            self.sessions
                .retain(|c| c.parent.is_none() || c.busy || !c.pending.is_empty());
        }
    }

    fn end_turn(&mut self, id: &str, parent: Option<&str>, at: UnixMs) {
        let i = self.index(id, parent, at);
        let s = &mut self.sessions[i];
        s.busy = false;
        s.pending.clear();
    }

    fn expire(&mut self, now: UnixMs) {
        for s in &mut self.sessions {
            if s.busy && s.pending.is_empty() && now.saturating_sub(s.last_at) > BUSY_EXPIRE_MS {
                s.busy = false;
            }
        }
    }

    fn derive(&self) -> Reported {
        let roots = || self.sessions.iter().filter(|s| s.parent.is_none());
        let children = || self.sessions.iter().filter(|s| s.parent.is_some());
        let blocked = self
            .sessions
            .iter()
            .filter(|s| s.status() == Status::Perm)
            .max_by_key(|s| s.last_at)
            .and_then(|s| s.pending.last());
        let status = if blocked.is_some() {
            Status::Perm
        } else if roots().chain(children()).any(|s| s.busy) {
            Status::Run
        } else {
            roots()
                .max_by_key(|s| s.last_at)
                .map_or(Status::Unknown, Session::status)
        };
        Reported {
            status,
            perm: blocked.map(|r| r.req.clone()),
            sub_agents: children()
                .map(|s| SubAgent {
                    id: SubAgentId::new(s.id.as_str()),
                    name: if s.name.is_empty() {
                        "subagent".to_owned()
                    } else {
                        s.name.clone()
                    },
                    status: s.status(),
                })
                .collect(),
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
            source: DetectSource::Hook,
            status: Some(now.status),
            sub_agents: Some(now.sub_agents.clone()),
            perm: now.perm.clone(),
            at,
        };
        self.reported = Some(now);
        vec![detection]
    }
}

fn str_field<'a>(payload: &'a Value, key: &str) -> Option<&'a str> {
    payload.get(key)?.as_str()
}

/// [`str_field`] cut to [`ID_MAX_CHARS`] characters.
fn id_field<'a>(payload: &'a Value, key: &str) -> Option<&'a str> {
    let s = str_field(payload, key)?;
    Some(
        s.char_indices()
            .nth(ID_MAX_CHARS)
            .map_or(s, |(at, _)| &s[..at]),
    )
}

fn clip(s: &str, max_chars: usize) -> String {
    s.chars().take(max_chars).collect()
}

impl Adapter for OpenCodeAdapter {
    fn name(&self) -> &'static str {
        "opencode-plugin"
    }

    fn agent(&self) -> AgentKind {
        AgentKind::OpenCode
    }

    fn source(&self) -> DetectSource {
        DetectSource::Hook
    }

    fn observe(&mut self, obs: &Observation) -> Vec<Detection> {
        match obs {
            Observation::Hook(HookObservation {
                agent: AgentKind::OpenCode,
                payload,
                at,
            }) => {
                if self.handle_hook(payload, *at) {
                    self.detection(*at)
                } else {
                    Vec::new()
                }
            }
            Observation::Tick { now } => {
                self.expire(*now);
                self.detection(*now)
            }
            Observation::Process(p)
                if p.foreground.as_deref().is_none_or(crate::screen::is_shell) =>
            {
                self.sessions.clear();
                self.detection(p.at)
            }
            _ => Vec::new(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::adapter::ProcessObservation;
    use serde_json::json;

    fn feed(a: &mut OpenCodeAdapter, at: UnixMs, payload: Value) -> Option<(Status, usize)> {
        let det = a.observe(&Observation::Hook(HookObservation {
            agent: AgentKind::OpenCode,
            payload,
            at,
        }));
        det.last()
            .map(|d| (d.status.unwrap(), d.sub_agents.as_ref().unwrap().len()))
    }

    fn ev(name: &str, session: &str) -> Value {
        json!({"hook_event_name": name, "session_id": session})
    }

    fn st(session: &str, status: &str) -> Value {
        json!({"hook_event_name": "SessionStatus", "session_id": session, "status": status})
    }

    #[test]
    fn records_and_strings_are_bounded() {
        let mut a = OpenCodeAdapter::new();
        let big = "ü".repeat(5_000);
        for n in 0..(MAX_RECORDS * 2) {
            feed(
                &mut a,
                n as u64,
                ev("SessionCreated", &format!("{n}-{big}")),
            );
        }
        assert_eq!(a.sessions.len(), MAX_RECORDS);
        assert!(
            a.sessions
                .iter()
                .all(|s| s.id.chars().count() <= ID_MAX_CHARS)
        );
        assert!(a.sessions[0].id.starts_with(&format!("{MAX_RECORDS}-")));

        for n in 0..(MAX_RECORDS * 2) {
            feed(
                &mut a,
                1_000 + n as u64,
                json!({"hook_event_name": "PermissionAsked", "session_id": "s",
                       "request_id": format!("r{n}"), "tool": big, "summary": big}),
            );
        }
        let s = a.sessions.last().unwrap();
        assert_eq!(s.pending.len(), MAX_RECORDS);
        assert_eq!(s.pending[0].req.tool.chars().count(), TOOL_MAX_CHARS);
        assert!(s.pending[0].req.summary.chars().count() <= SUMMARY_MAX_CHARS);
    }

    #[test]
    fn turn_lifecycle_idle_run_done() {
        let mut a = OpenCodeAdapter::new();
        assert_eq!(
            feed(&mut a, 1, ev("SessionCreated", "r")),
            Some((Status::Idle, 0))
        );
        assert_eq!(feed(&mut a, 2, st("r", "busy")), Some((Status::Run, 0)));
        assert_eq!(feed(&mut a, 3, st("r", "busy")), None);
        assert_eq!(feed(&mut a, 4, st("r", "retry")), None);
        assert_eq!(feed(&mut a, 5, st("r", "idle")), Some((Status::Done, 0)));
        assert_eq!(
            feed(&mut a, 6, ev("SessionIdle", "r")),
            None,
            "deprecated twin changes nothing"
        );
        assert_eq!(feed(&mut a, 7, st("r", "busy")), Some((Status::Run, 0)));
    }

    #[test]
    fn error_shows_fail_once_idle_and_a_retry_clears_it() {
        let mut a = OpenCodeAdapter::new();
        feed(&mut a, 1, st("r", "busy"));
        assert_eq!(feed(&mut a, 2, ev("SessionError", "r")), None, "still busy");
        assert_eq!(feed(&mut a, 3, st("r", "idle")), Some((Status::Fail, 0)));
        feed(&mut a, 4, st("r", "busy"));
        feed(&mut a, 5, ev("SessionError", "r"));
        assert_eq!(feed(&mut a, 6, st("r", "retry")), None);
        assert_eq!(
            feed(&mut a, 7, st("r", "idle")),
            Some((Status::Done, 0)),
            "recovered"
        );
    }

    #[test]
    fn permission_and_question_requests() {
        let mut a = OpenCodeAdapter::new();
        feed(&mut a, 1, st("r", "busy"));
        let det = a.observe(&Observation::Hook(HookObservation {
            agent: AgentKind::OpenCode,
            payload: json!({"hook_event_name": "PermissionAsked", "session_id": "r", "request_id": "p1", "tool": "bash", "summary": "npm test"}),
            at: 2,
        }));
        assert_eq!(det[0].status, Some(Status::Perm));
        assert_eq!(
            det[0].perm,
            Some(PermRequest {
                tool: "bash".into(),
                summary: "npm test".into(),
                always: None,
            })
        );
        let reply =
            json!({"hook_event_name": "PermissionReplied", "session_id": "r", "request_id": "p1"});
        assert_eq!(feed(&mut a, 3, reply), Some((Status::Run, 0)));
        let q = json!({"hook_event_name": "QuestionAsked", "session_id": "r", "request_id": "q1", "summary": "Which db?"});
        let det = a.observe(&Observation::Hook(HookObservation {
            agent: AgentKind::OpenCode,
            payload: q,
            at: 4,
        }));
        assert_eq!(det[0].perm.as_ref().unwrap().tool, "question");
        // The turn ending drops an unanswered request.
        assert_eq!(feed(&mut a, 5, st("r", "idle")), Some((Status::Done, 0)));
        // A reply without an id closes whatever is open.
        feed(
            &mut a,
            6,
            json!({"hook_event_name": "PermissionAsked", "session_id": "r", "request_id": "p2"}),
        );
        assert_eq!(
            feed(&mut a, 7, ev("QuestionRejected", "r")),
            Some((Status::Done, 0))
        );
    }

    #[test]
    fn child_sessions_are_sub_agents_that_hold_the_card_open() {
        let mut a = OpenCodeAdapter::new();
        feed(&mut a, 1, st("r", "busy"));
        let child = |name: &str, at, extra: Value| {
            let mut p = json!({"hook_event_name": name, "session_id": "c1", "parent_id": "r"});
            p.as_object_mut()
                .unwrap()
                .extend(extra.as_object().unwrap().clone());
            (at, p)
        };
        let (at, p) = child("SessionCreated", 2, json!({"title": "Explore repo"}));
        assert_eq!(feed(&mut a, at, p), Some((Status::Run, 1)));
        let (at, p) = child("SessionStatus", 3, json!({"status": "busy"}));
        feed(&mut a, at, p);
        // Lead finishes first: the child keeps the session running.
        assert_eq!(feed(&mut a, 4, st("r", "idle")), None);
        // A child asking blocks the session.
        let (at, p) = child(
            "PermissionAsked",
            5,
            json!({"request_id": "x", "tool": "edit"}),
        );
        assert_eq!(feed(&mut a, at, p), Some((Status::Perm, 1)));
        let (at, p) = child("PermissionReplied", 6, json!({"request_id": "x"}));
        assert_eq!(feed(&mut a, at, p), Some((Status::Run, 1)));
        let (at, p) = child("SessionStatus", 7, json!({"status": "idle"}));
        let det = a.observe(&Observation::Hook(HookObservation {
            agent: AgentKind::OpenCode,
            payload: p,
            at,
        }));
        assert_eq!(det[0].status, Some(Status::Done));
        let subs = det[0].sub_agents.as_ref().unwrap();
        assert_eq!(
            (subs[0].name.as_str(), subs[0].status),
            ("Explore repo", Status::Done)
        );
        // The next root turn drops the finished child.
        assert_eq!(feed(&mut a, 8, st("r", "busy")), Some((Status::Run, 0)));
    }

    #[test]
    fn child_status_before_creation_event_is_still_a_child() {
        let mut a = OpenCodeAdapter::new();
        feed(&mut a, 1, st("r", "busy"));
        let p = json!({"hook_event_name": "SessionStatus", "session_id": "c", "parent_id": "r", "status": "busy"});
        assert_eq!(feed(&mut a, 2, p), Some((Status::Run, 1)));
    }

    #[test]
    fn deleted_sessions_vanish_and_junk_is_ignored() {
        let mut a = OpenCodeAdapter::new();
        feed(&mut a, 1, st("r", "busy"));
        assert_eq!(
            feed(&mut a, 2, ev("SessionDeleted", "r")),
            Some((Status::Unknown, 0))
        );
        for junk in [
            json!({}),
            json!({"hook_event_name": "SessionStatus"}),
            json!({"hook_event_name": "SessionStatus", "session_id": "r", "status": "weird"}),
            json!({"hook_event_name": "Whatever", "session_id": "r"}),
            json!({"hook_event_name": 7, "session_id": "r"}),
        ] {
            assert_eq!(feed(&mut a, 3, junk), None);
        }
        // Another agent's hook payloads are not for this adapter.
        let other = Observation::Hook(HookObservation {
            agent: AgentKind::ClaudeCode,
            payload: st("r", "busy"),
            at: 4,
        });
        assert!(a.observe(&other).is_empty());
    }

    #[test]
    fn silent_busy_sessions_expire_on_tick_and_process_exit_clears() {
        let mut a = OpenCodeAdapter::new();
        feed(&mut a, 1000, st("r", "busy"));
        assert!(
            a.observe(&Observation::Tick {
                now: 1000 + BUSY_EXPIRE_MS
            })
            .is_empty()
        );
        let det = a.observe(&Observation::Tick {
            now: 1001 + BUSY_EXPIRE_MS,
        });
        assert_eq!(det[0].status, Some(Status::Done));
        feed(&mut a, 3_000_000, st("r", "busy"));
        let exit = Observation::Process(ProcessObservation {
            foreground: None,
            exit_code: Some(0),
            at: 3_000_001,
        });
        let det = a.observe(&exit);
        assert_eq!(
            (det[0].status, det[0].sub_agents.as_ref().map(Vec::len)),
            (Some(Status::Unknown), Some(0))
        );
    }

    #[test]
    fn a_keep_alive_busy_keeps_a_long_quiet_turn_running() {
        let mut a = OpenCodeAdapter::new();
        feed(&mut a, 1000, st("r", "busy"));
        // The plugin repeats busy every few minutes: no change to report, but the clock moves.
        assert_eq!(
            feed(&mut a, 1000 + BUSY_EXPIRE_MS - 1, st("r", "busy")),
            None
        );
        let det = a.observe(&Observation::Tick {
            now: 1000 + BUSY_EXPIRE_MS + 60_000,
        });
        assert!(
            det.is_empty(),
            "still running an hour in, as long as the keep-alives arrive"
        );
        // Without them the session is eventually written off.
        let det = a.observe(&Observation::Tick {
            now: 2000 + 2 * BUSY_EXPIRE_MS,
        });
        assert_eq!(det[0].status, Some(Status::Done));
    }

    #[test]
    fn default_adapter_is_for_opencode_only() {
        let id = SessionId::new("s");
        assert!(default_adapter(&id, AgentKind::OpenCode).is_some());
        assert!(default_adapter(&id, AgentKind::Codex).is_none());
    }
}
