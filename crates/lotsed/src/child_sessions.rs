//! Child sessions: an agent (through `lotse-hook mcp`) starts, follows, messages and closes other
//! Lotse sessions. [`crate::child_policy`] decides what is allowed; this module holds the open
//! approval cards ([`StartRequests`]) and runs the allowed requests.
//!
//! A start never happens on the agent's word alone: the request becomes an
//! `Event::SessionStartRequested` card in the app, the agent's tool call waits, and
//! `Request::ResolveSessionStart` (an app client; mcp clients are refused) starts or declines it.
//! Every decision is logged at `info` with the requesting session: the audit trail.

use crate::child_policy::{self, Fact};
use crate::hub::ClientTx;
use crate::session::now_ms;
use crate::state::{State, not_found};
use lotse_proto::{
    ErrorCode, Event, NewSession, ProtoError, Request, Response, SessionId, Settings, Status,
};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::sync::oneshot;

/// How long a start request waits for the user.
pub const START_TIMEOUT: Duration = Duration::from_secs(15 * 60);
/// A message reaches the agent as one bracketed paste; the Return follows after this pause.
const RETURN_DELAY: Duration = Duration::from_millis(150);
/// Turns of a child's transcript an agent gets when it names no limit.
const DEFAULT_TRANSCRIPT_TURNS: u32 = 10;

/// The open approval cards.
#[derive(Default)]
pub struct StartRequests {
    open: Mutex<HashMap<String, Pending>>,
    /// Held from the last limit check to the start, so two approved requests cannot both pass it.
    gate: tokio::sync::Mutex<()>,
}

struct Pending {
    client_id: u64,
    parent: SessionId,
    answer: oneshot::Sender<bool>,
}

impl StartRequests {
    fn lock(&self) -> std::sync::MutexGuard<'_, HashMap<String, Pending>> {
        self.open.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// A new card for `parent`'s agent, asked over connection `client_id`.
    pub fn open(&self, client_id: u64, parent: SessionId) -> (String, oneshot::Receiver<bool>) {
        let id = format!("r-{}", &uuid::Uuid::new_v4().simple().to_string()[..10]);
        let (answer, rx) = oneshot::channel();
        self.lock().insert(
            id.clone(),
            Pending {
                client_id,
                parent,
                answer,
            },
        );
        (id, rx)
    }

    /// Answer a card; `false` when there is no such open card (answered, withdrawn or never there).
    pub fn resolve(&self, id: &str, approve: bool) -> bool {
        match self.lock().remove(id) {
            Some(p) => p.answer.send(approve).is_ok(),
            None => false,
        }
    }

    /// Drop a card nobody answers any more.
    pub fn withdraw(&self, id: &str) {
        self.lock().remove(id);
    }

    /// The agent's connection ended: its cards are withdrawn, their tool calls are gone.
    /// Returns the ids of the withdrawn cards.
    pub fn client_gone(&self, client_id: u64) -> Vec<String> {
        self.take(|p| p.client_id == client_id)
    }

    /// `parent` was closed: its open cards are withdrawn (the waiting calls see a closed channel).
    /// Returns the ids of the withdrawn cards.
    pub fn withdraw_parent(&self, parent: &SessionId) -> Vec<String> {
        self.take(|p| &p.parent == parent)
    }

    fn take(&self, pred: impl Fn(&Pending) -> bool) -> Vec<String> {
        let mut open = self.lock();
        let ids: Vec<String> = open
            .iter()
            .filter(|(_, p)| pred(p))
            .map(|(id, _)| id.clone())
            .collect();
        for id in &ids {
            open.remove(id);
        }
        ids
    }

    pub async fn enter(&self) -> tokio::sync::MutexGuard<'_, ()> {
        self.gate.lock().await
    }

    /// Open cards of `parent`.
    pub fn count_for(&self, parent: &SessionId) -> usize {
        self.lock().values().filter(|p| &p.parent == parent).count()
    }
}

/// The Settings toggle "Agents may start sessions without asking" (off by default): a start in
/// the requesting session's project then skips the card.
fn start_without_asking(settings: &Settings) -> bool {
    settings.agents_start_sessions_without_asking
}

fn refuse(code: ErrorCode, message: impl Into<String>) -> Response {
    Response::Error(ProtoError {
        code,
        message: message.into(),
    })
}

impl State {
    /// What an mcp client (`Hello.client` = `mcp` / `mcp:<session>`) gets: the allowlist of
    /// [`child_policy`] and nothing else.
    pub(crate) async fn handle_mcp(
        self: &Arc<Self>,
        client: &Arc<ClientTx>,
        caller: Option<SessionId>,
        req: Request,
    ) -> Response {
        if matches!(req, Request::Ping) {
            return Response::Pong;
        }
        let method = req.method();
        let Some(caller) = caller else {
            return refuse(
                ErrorCode::Unauthorized,
                "this connection names no session: use client `mcp:<session id>`",
            );
        };
        let facts = self.facts();
        if let Err(e) = child_policy::check(&req, &caller, &facts, self.starts.count_for(&caller)) {
            tracing::info!("agent {caller}: {method} refused: {}", e.message);
            return Response::Error(e);
        }
        match req {
            Request::ListSessions { project_id } => {
                let visible = child_policy::visible(&caller, &facts);
                let mut sessions = self.session_list(project_id.as_ref());
                sessions.retain(|s| visible.contains(&s.id));
                Response::Sessions(sessions)
            }
            Request::CreateSession(ns) => self.start_for_agent(client, &caller, ns).await,
            Request::GetTranscript { session_id, limit } => {
                let limit = limit
                    .unwrap_or(DEFAULT_TRANSCRIPT_TURNS)
                    .clamp(1, child_policy::MAX_TRANSCRIPT_TURNS);
                self.get_transcript(&session_id, Some(limit)).await
            }
            Request::SendInput { session_id, text } => {
                tracing::info!("agent {caller}: message to {session_id}");
                self.message_session(&session_id, &text)
            }
            Request::CloseSession { session_id } => {
                tracing::info!("agent {caller}: closes {session_id}");
                self.close_session(&session_id)
            }
            _ => refuse(ErrorCode::Unauthorized, "not available to agents"),
        }
    }

    /// Tell the apps these approval cards are gone.
    pub(crate) fn announce_withdrawn(&self, ids: Vec<String>) {
        for id in ids {
            self.hub.broadcast(Event::SessionStartWithdrawn { id });
        }
    }

    /// `parent` was closed: its open cards are withdrawn and its children become top-level
    /// sessions (the desktop app's `detached`).
    pub(crate) fn parent_closed(&self, parent: &SessionId) {
        self.announce_withdrawn(self.starts.withdraw_parent(parent));
        for h in self.handles() {
            if let Some(meta) = h.detach_from_parent(parent) {
                self.hub.broadcast(Event::SessionUpdated(meta));
            }
        }
    }

    /// Everything the policy needs to know of the open sessions.
    pub(crate) fn facts(&self) -> Vec<Fact> {
        self.handles()
            .iter()
            .map(|h| {
                let s = h.snapshot();
                Fact {
                    id: s.id,
                    project_id: s.project_id,
                    parent: s.parent_session,
                    status: s.status,
                    alive: h.is_alive(),
                }
            })
            .collect()
    }

    /// `start_session`: the card, the wait, the start.
    async fn start_for_agent(
        self: &Arc<Self>,
        client: &Arc<ClientTx>,
        caller: &SessionId,
        mut ns: NewSession,
    ) -> Response {
        match self.db().get_project(&ns.project_id) {
            Ok(Some(_)) => {}
            Ok(None) => return not_found("project"),
            Err(e) => {
                tracing::error!("{e}");
                return refuse(ErrorCode::Internal, "internal error");
            }
        }
        ns.parent_session = Some(caller.clone());
        ns.requested_by_agent = true;
        // what the card shows is what gets typed: no control characters
        ns.first_prompt = match ns
            .first_prompt
            .as_deref()
            .map(child_policy::sanitize_prompt)
        {
            Some(Ok(p)) => Some(p),
            Some(Err(e)) => return Response::Error(e),
            None => return refuse(ErrorCode::InvalidRequest, "a prompt is required"),
        };
        let caller_project = self
            .session(caller)
            .map(|h| h.snapshot().project_id)
            .unwrap_or_else(|| ns.project_id.clone());
        let prompt = ns.first_prompt.clone().unwrap_or_default();
        if child_policy::needs_card(&ns, &caller_project, start_without_asking(&self.settings())) {
            let (id, answer) = self.starts.open(client.id, caller.clone());
            // requests that arrived together all passed the check before any was open
            if self.starts.count_for(caller) > child_policy::MAX_PENDING_PER_SESSION {
                self.starts.withdraw(&id);
                return refuse(ErrorCode::Conflict, "too many open start requests");
            }
            tracing::info!("agent {caller}: asks to start a session ({id})");
            self.hub.broadcast(Event::SessionStartRequested {
                id: id.clone(),
                parent_session: caller.clone(),
                project_id: ns.project_id.clone(),
                agent: ns.agent,
                title: ns.title.clone(),
                prompt,
                worktree: ns.worktree.clone(),
                at: now_ms(),
            });
            match tokio::time::timeout(START_TIMEOUT, answer).await {
                Ok(Ok(true)) => tracing::info!("agent {caller}: start {id} approved"),
                Ok(Ok(false)) => {
                    tracing::info!("agent {caller}: start {id} declined");
                    return refuse(
                        ErrorCode::Conflict,
                        "the user declined to start the session",
                    );
                }
                Ok(Err(_)) => return refuse(ErrorCode::Unavailable, "the request was withdrawn"),
                Err(_) => {
                    self.starts.withdraw(&id);
                    self.hub
                        .broadcast(Event::SessionStartWithdrawn { id: id.clone() });
                    tracing::info!("agent {caller}: start {id} timed out");
                    return refuse(ErrorCode::Unavailable, "nobody answered the start request");
                }
            }
        } else {
            tracing::info!("agent {caller}: starts a session without asking (setting)");
        }
        // The sessions may have changed while the card was open; one start at a time from here to
        // the new session, so approved requests cannot all pass the limit together.
        let _one_at_a_time = self.starts.enter().await;
        let again = Request::CreateSession(ns.clone());
        if let Err(e) = child_policy::check(&again, caller, &self.facts(), 0) {
            return Response::Error(e);
        }
        self.create_session(ns).await
    }

    /// Type `text` into an idle agent as one bracketed paste and press Return. The status is
    /// checked again here, at the moment of typing.
    fn message_session(&self, id: &SessionId, text: &str) -> Response {
        let text = match child_policy::sanitize_message(text) {
            Ok(t) => t,
            Err(e) => return Response::Error(e),
        };
        let Some(h) = self.session(id) else {
            return not_found("session");
        };
        if !matches!(h.snapshot().status, Status::Idle | Status::Done) {
            return refuse(ErrorCode::Conflict, "the session is not idle");
        }
        // like the app's composer: line breaks inside a paste are Returns
        let body = text.replace("\r\n", "\n").replace('\n', "\r");
        let paste = format!("\x1b[200~{body}\x1b[201~");
        if let Err(e) = h.inject(paste.as_bytes()) {
            return refuse(ErrorCode::Conflict, e.to_string());
        }
        let handle = h.clone();
        let spawned = std::thread::Builder::new()
            .name("lotse-message".into())
            .spawn(move || {
                std::thread::sleep(RETURN_DELAY);
                let _ = handle.inject(b"\r");
            });
        if let Err(e) = spawned {
            tracing::warn!("message not submitted: {e}");
        }
        Response::Ok
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sid(s: &str) -> SessionId {
        SessionId::new(s)
    }

    #[test]
    fn a_card_answers_once() {
        let starts = StartRequests::default();
        let (id, mut rx) = starts.open(1, sid("a"));
        assert!(starts.resolve(&id, true));
        assert_eq!(rx.try_recv(), Ok(true));
        assert!(!starts.resolve(&id, true), "already answered");
        assert!(!starts.resolve("r-unknown", false));
    }

    #[test]
    fn a_connection_that_ends_withdraws_its_cards_only() {
        let starts = StartRequests::default();
        let (mine, mut mine_rx) = starts.open(1, sid("a"));
        let (theirs, _keep) = starts.open(2, sid("b"));
        starts.client_gone(1);
        assert!(!starts.resolve(&mine, true), "withdrawn");
        assert!(
            mine_rx.try_recv().is_err(),
            "the waiting call sees a closed channel"
        );
        assert!(starts.resolve(&theirs, true));
    }

    #[test]
    fn open_cards_are_counted_per_requesting_session() {
        let starts = StartRequests::default();
        let (a1, _r1) = starts.open(1, sid("a"));
        let (_a2, _r2) = starts.open(1, sid("a"));
        let (_b, _r3) = starts.open(2, sid("b"));
        assert_eq!(starts.count_for(&sid("a")), 2);
        assert_eq!(starts.count_for(&sid("b")), 1);
        starts.withdraw(&a1);
        assert_eq!(starts.count_for(&sid("a")), 1);
        let ids: std::collections::HashSet<_> =
            (0..50).map(|_| starts.open(3, sid("c")).0).collect();
        assert_eq!(ids.len(), 50, "ids are unique");
    }

    #[test]
    fn the_toggle_follows_the_setting_and_is_off_by_default() {
        let mut s = Settings::default();
        assert!(!start_without_asking(&s));
        s.agents_start_sessions_without_asking = true;
        assert!(start_without_asking(&s));
    }

    #[test]
    fn cards_of_a_closed_parent_are_returned_for_withdrawal() {
        let starts = StartRequests::default();
        let (a, _r1) = starts.open(1, sid("a"));
        let (b, _r2) = starts.open(2, sid("b"));
        assert_eq!(starts.withdraw_parent(&sid("a")), vec![a]);
        assert_eq!(starts.count_for(&sid("a")), 0);
        assert_eq!(starts.client_gone(2), vec![b]);
    }
}
