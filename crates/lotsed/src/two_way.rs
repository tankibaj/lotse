//! The Claude mod, two-way (beta M4): what the app can do *through* the mod instead of the keyboard.
//!
//! - **Allow / Deny** (`Request::ResolvePermission`): while the mod holds the session's
//!   `PermissionRequest` hook (`perm_hold` … `perm_release` status lines; the hook is cut at 10 s, so
//!   the mod holds 7.5 s and this side [`HOLD_WINDOW`] 7 s), the answer goes to the mod as
//!   `ModCommand::Answer` and the hook returns it. Outside the window, for Always, for another prompt
//!   than the held one, and with no mod, the keystroke path (`perm_answer`) answers as before.
//! - **Composer send**: `Request::SendInput` whose text starts with [`SUBMIT_MARKER`] goes to the mod as
//!   `ModCommand::Submit` (`$.prompt.submit`). Without a mod, or when the mod reports `submit_result`
//!   `ok: false`, the daemon types it as a bracketed paste and a Return, what the composer did before.
//!   The marker is an in-band stand-in for a `Request::SubmitPrompt` the frozen proto 5 does not have.
//!
//! Only the app decides and submits: a connection that said hello as the mod or the hook is refused.
//! The answer reaches the one connection the mod attached with, for the session's own open prompt.
//! Nothing here allows anything by itself.

use crate::hub::PermDecisionRecord;
use crate::mod_source::MOD_CLIENT;
use crate::session::{SessionError, SessionHandle};
use crate::state::State;
use lotse_proto::{
    ErrorCode, ModCommand, PermDecision, PermVia, ProtoError, Request, Response, SessionId, Status,
};
use serde_json::Value;
use std::collections::{HashMap, VecDeque};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// Starts the text of a composer send for the mod (`app/src/composer/send.ts` has the same string).
pub const SUBMIT_MARKER: &str = "\u{0}lotse-submit\u{0}";

/// How long after `perm_hold` an answer still goes to the mod. The mod holds the hook 7.5 s.
pub const HOLD_WINDOW: Duration = Duration::from_millis(7000);
/// How long a submit waits for the mod's `submit_result` before it is forgotten.
const RESULT_WINDOW: Duration = Duration::from_secs(10);
/// The mod refuses longer texts (it counts UTF-16 units, so this is the safe side of 100 000).
const MAX_PROMPT_CHARS: usize = 50_000;
const MAX_PENDING: usize = 8;
const MAX_TOOL_CHARS: usize = 64;
/// Pause between the pasted text and the Return that submits it (as the composer's own).
const SUBMIT_DELAY: Duration = Duration::from_millis(150);

struct Hold {
    tool: String,
    since: Instant,
}

#[derive(Default)]
struct Inner {
    holds: HashMap<SessionId, Hold>,
    /// Texts handed to a session's mod that have not been reported on yet, oldest first.
    submits: HashMap<SessionId, VecDeque<(String, Instant)>>,
}

/// Open holds and unreported submits, per session.
#[derive(Default)]
pub struct TwoWay {
    inner: Mutex<Inner>,
}

impl TwoWay {
    fn lock(&self) -> std::sync::MutexGuard<'_, Inner> {
        self.inner.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// The mod holds a `PermissionRequest` for `tool` (replaces an earlier hold of the session).
    pub fn hold(&self, session: SessionId, tool: &str, now: Instant) {
        let mut g = self.lock();
        g.holds
            .retain(|_, h| now.saturating_duration_since(h.since) < HOLD_WINDOW);
        g.holds.insert(
            session,
            Hold {
                tool: tool.chars().take(MAX_TOOL_CHARS).collect(),
                since: now,
            },
        );
    }

    /// The hold ended: answered, timed out, or the dialog took over.
    pub fn release(&self, session: &SessionId) {
        self.lock().holds.remove(session);
    }

    /// Use up the hold of `session` for a prompt about `tool`: `true` only inside the window and for
    /// the tool the mod held (an answer names no agent, so the tool is the check that it is the same
    /// request). A hold for another tool stays, its own prompt may still come.
    pub fn take_hold(&self, session: &SessionId, tool: &str, now: Instant) -> bool {
        let mut g = self.lock();
        let Some(h) = g.holds.get(session) else {
            return false;
        };
        let fresh = now.saturating_duration_since(h.since) < HOLD_WINDOW;
        let same_tool = h.tool == tool;
        if !fresh || same_tool {
            g.holds.remove(session);
        }
        fresh && same_tool
    }

    /// `text` went to the mod of `session`; its `submit_result` is due.
    pub fn submitted(&self, session: SessionId, text: String, now: Instant) {
        let mut g = self.lock();
        g.submits.retain(|_, q| {
            q.back()
                .is_some_and(|(_, at)| now.saturating_duration_since(*at) < RESULT_WINDOW)
        });
        let queue = g.submits.entry(session).or_default();
        if queue.len() == MAX_PENDING {
            queue.pop_front();
        }
        queue.push_back((text, now));
    }

    /// The mod reported on the oldest submit of `session` still waiting; its text, unless it is too old.
    pub fn reported(&self, session: &SessionId, now: Instant) -> Option<String> {
        let mut g = self.lock();
        let queue = g.submits.get_mut(session)?;
        while let Some((text, at)) = queue.pop_front() {
            if now.saturating_duration_since(at) < RESULT_WINDOW {
                return Some(text);
            }
        }
        None
    }
}

fn refuse(code: ErrorCode, message: &str) -> Response {
    Response::Error(ProtoError {
        code,
        message: message.into(),
    })
}

/// Connections that never decide or submit for the user, however they got a token.
fn is_agent_side(client: &str) -> bool {
    client == MOD_CLIENT || client == "hook"
}

/// A status line of the mod that is about the two-way part (`state.rs` calls this for every one).
pub(crate) fn observe(state: &State, session: &SessionId, line: &Value) {
    let now = Instant::now();
    match line.get("t").and_then(Value::as_str) {
        Some("perm_hold") => {
            if let Some(tool) = line.get("tool").and_then(Value::as_str) {
                state.two_way().hold(session.clone(), tool, now);
            }
        }
        Some("perm_release") => state.two_way().release(session),
        Some("submit_result") => {
            let text = state.two_way().reported(session, now);
            if line.get("ok").and_then(Value::as_bool) == Some(false)
                && let (Some(text), Some(handle)) = (text, state.session(session))
            {
                let _ = type_prompt(&handle, &text);
            }
        }
        _ => {}
    }
}

/// The requests of the app this module answers (`State::handle` asks first): `Some(response)` when it
/// did, `None` to let the request take its usual path.
pub(crate) fn intercept(state: &State, client: &str, request: &Request) -> Option<Response> {
    match request {
        Request::ResolvePermission {
            session_id,
            decision,
            via,
            prompt_id,
        } => resolve(
            state,
            client,
            session_id,
            *decision,
            *via,
            prompt_id.as_deref(),
        ),
        Request::SendInput { session_id, text } => submit(state, client, session_id, text),
        _ => None,
    }
}

/// `Request::ResolvePermission`: `Some(response)` when this side handled it (an answer through the
/// mod, or a refusal); `None` to take the keystroke path.
fn resolve(
    state: &State,
    client: &str,
    session: &SessionId,
    decision: PermDecision,
    via: Option<PermVia>,
    prompt_id: Option<&str>,
) -> Option<Response> {
    if is_agent_side(client) {
        return Some(refuse(
            ErrorCode::InvalidRequest,
            "only the app decides a permission",
        ));
    }
    if decision == PermDecision::Always {
        return None;
    }
    let snapshot = state.session(session)?.snapshot();
    let at_prompt = snapshot.status == Status::Perm;
    let perm = snapshot.perm.filter(|_| at_prompt)?;
    if prompt_id.is_some_and(|id| perm.id.as_deref() != Some(id)) {
        return None; // the keystroke path says "stale prompt"
    }
    if !state
        .two_way()
        .take_hold(session, &perm.tool, Instant::now())
    {
        return None;
    }
    let answer = ModCommand::Answer {
        prompt_id: perm.id.clone(),
        decision,
    };
    if !state.send_mod_command(session, answer) {
        return None;
    }
    state.hub.publish_decision(&PermDecisionRecord {
        session_id: session.clone(),
        prompt_id: perm.id,
        decision,
        via,
    });
    Some(Response::Ok)
}

/// `Request::SendInput`: `Some(response)` for a composer send ([`SUBMIT_MARKER`]), `None` for any
/// other input.
fn submit(state: &State, client: &str, session: &SessionId, text: &str) -> Option<Response> {
    let body = text.strip_prefix(SUBMIT_MARKER)?;
    if is_agent_side(client) {
        return Some(refuse(
            ErrorCode::InvalidRequest,
            "only the app sends prompts",
        ));
    }
    let Some(handle) = state.session(session) else {
        return Some(refuse(ErrorCode::NotFound, "session not found"));
    };
    let body = clean(body);
    if body.is_empty() {
        return Some(Response::Ok);
    }
    if body.chars().count() > MAX_PROMPT_CHARS {
        return Some(refuse(ErrorCode::InvalidRequest, "prompt is too long"));
    }
    let sent = state.send_mod_command(session, ModCommand::Submit { text: body.clone() });
    if sent {
        state
            .two_way()
            .submitted(session.clone(), body, Instant::now());
        return Some(Response::Ok);
    }
    Some(match type_prompt(&handle, &body) {
        Ok(()) => Response::Ok,
        Err(e) => refuse(ErrorCode::Conflict, &e.to_string()),
    })
}

/// Line breaks as `\n`, no control characters but tab, no trailing line breaks.
fn clean(text: &str) -> String {
    text.replace("\r\n", "\n")
        .replace('\r', "\n")
        .chars()
        .filter(|c| !c.is_control() || matches!(c, '\n' | '\t'))
        .collect::<String>()
        .trim_end_matches('\n')
        .to_owned()
}

/// What the composer typed before the mod: the text as one bracketed paste (line breaks as Returns),
/// then, as its own write after [`SUBMIT_DELAY`], the Return.
fn paste_bytes(text: &str) -> String {
    format!("\x1b[200~{}\x1b[201~", text.replace('\n', "\r"))
}

fn type_prompt(handle: &Arc<SessionHandle>, text: &str) -> Result<(), SessionError> {
    handle.write_input(paste_bytes(text).as_bytes())?;
    let handle = handle.clone();
    let spawned = std::thread::Builder::new()
        .name("lotse-submit".into())
        .spawn(move || {
            std::thread::sleep(SUBMIT_DELAY);
            let _ = handle.write_input(b"\r");
        });
    if let Err(e) = spawned {
        tracing::warn!("Return of a typed prompt not sent: {e}");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(base: Instant, ms: u64) -> Instant {
        base + Duration::from_millis(ms)
    }

    #[test]
    fn a_hold_answers_once_inside_the_window_for_its_own_tool() {
        let (tw, s, t0) = (TwoWay::default(), SessionId::new("s"), Instant::now());
        tw.hold(s.clone(), "Bash", t0);
        assert!(
            !tw.take_hold(&s, "Edit", at(t0, 10)),
            "another tool's prompt"
        );
        assert!(tw.take_hold(&s, "Bash", at(t0, 20)));
        assert!(!tw.take_hold(&s, "Bash", at(t0, 30)), "spent");
    }

    #[test]
    fn a_hold_is_over_before_the_hook_is_cut() {
        let (tw, s, t0) = (TwoWay::default(), SessionId::new("s"), Instant::now());
        tw.hold(s.clone(), "Bash", t0);
        let just_in_time = HOLD_WINDOW.as_millis() as u64 - 1;
        assert!(tw.take_hold(&s, "Bash", at(t0, just_in_time)));
        tw.hold(s.clone(), "Bash", t0);
        assert!(!tw.take_hold(&s, "Bash", at(t0, HOLD_WINDOW.as_millis() as u64)));
        assert!(
            HOLD_WINDOW < Duration::from_millis(7500),
            "under the mod's own wait"
        );
    }

    #[test]
    fn a_released_hold_and_another_sessions_hold_do_not_answer() {
        let (tw, t0) = (TwoWay::default(), Instant::now());
        let (a, b) = (SessionId::new("a"), SessionId::new("b"));
        tw.hold(a.clone(), "Bash", t0);
        tw.release(&a);
        assert!(!tw.take_hold(&a, "Bash", t0));
        tw.hold(b.clone(), "Bash", t0);
        assert!(!tw.take_hold(&a, "Bash", t0));
    }

    #[test]
    fn results_come_back_in_the_order_of_the_submits_and_old_ones_are_forgotten() {
        let (tw, s, t0) = (TwoWay::default(), SessionId::new("s"), Instant::now());
        tw.submitted(s.clone(), "one".into(), t0);
        tw.submitted(s.clone(), "two".into(), at(t0, 5));
        assert_eq!(tw.reported(&s, at(t0, 50)).as_deref(), Some("one"));
        assert_eq!(tw.reported(&s, at(t0, 60)).as_deref(), Some("two"));
        assert_eq!(tw.reported(&s, at(t0, 70)), None);
        tw.submitted(s.clone(), "late".into(), t0);
        let past = RESULT_WINDOW.as_millis() as u64;
        assert_eq!(tw.reported(&s, at(t0, past)), None);
    }

    #[test]
    fn the_text_is_cleaned_and_pasted_as_one_block() {
        assert_eq!(clean("a\r\nb\rc\u{7}\u{0}\t!\n\n"), "a\nb\nc\t!");
        assert_eq!(clean("\n\n"), "");
        assert_eq!(paste_bytes("a\nb"), "\x1b[200~a\rb\x1b[201~");
    }

    #[test]
    fn the_marker_matches_the_composers() {
        let composer = include_str!("../../../app/src/composer/send.ts");
        assert!(
            composer.contains("\\u0000lotse-submit\\u0000"),
            "app/src/composer/send.ts must carry the same marker"
        );
    }
}
