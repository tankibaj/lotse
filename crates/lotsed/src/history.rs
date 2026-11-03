//! Beta (wp06): records status spans and permission events into the database, from two listeners
//! on the hub ([`Hub::observe`] for events, [`Hub::observe_decisions`] for typed answers).
//!
//! - Status: the session's own lane follows `status_changed` (the first span opens at
//!   `session_created`), sub-agent lanes follow `sub_agents_changed` / `sub_agent_status`. A span
//!   ends when the status changes, the session is closed or exits, or the daemon restarts.
//! - Permission: a `perm` status opens a `perm_events` row (once per prompt id); the answer typed
//!   by Lotse stamps `decision` and `via`; a prompt that goes away without one (answered in the
//!   terminal) only gets `decided_at`.
//! - Retention: 30 days, pruned at install and then about once a day.
//!
//! Every write is idempotent (same status, same prompt id), so installing twice, or a repeated
//! event, records nothing twice. A failed write is logged and dropped: history never blocks a status.

use crate::db::Db;
use crate::hub::{Hub, PermDecisionRecord};
use crate::session::now_ms;
use lotse_proto::{Event, Status};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

const DAY_MS: u64 = 86_400_000;
/// History older than this is dropped.
pub const RETENTION_MS: u64 = 30 * DAY_MS;

/// Close what the previous daemon run left open, prune, and start listening.
pub fn install(hub: &Hub, db: Arc<Db>) {
    let now = now_ms();
    match db.close_all_open(now) {
        Ok((0, 0)) => {}
        Ok((spans, asked)) => {
            tracing::info!(
                "history: closed {spans} span(s) and {asked} question(s) of the previous run"
            );
        }
        Err(e) => tracing::warn!("history: closing open spans: {e}"),
    }
    let recorder = Arc::new(Recorder {
        db,
        pruned_at: AtomicU64::new(0),
    });
    recorder.prune(now);
    let events = recorder.clone();
    hub.observe(Box::new(move |e| events.on_event(e)));
    hub.observe_decisions(Box::new(move |d| recorder.on_decision(d)));
}

struct Recorder {
    db: Arc<Db>,
    /// When history was last pruned (0 = not yet).
    pruned_at: AtomicU64,
}

impl Recorder {
    fn on_event(&self, event: &Event) {
        let now = now_ms();
        let result = match event {
            Event::SessionCreated(s) => {
                self.db.transition_span(&s.id, None, s.status, s.created_at)
            }
            Event::StatusChanged {
                session_id,
                status,
                perm,
                at,
                ..
            } => self
                .db
                .transition_span(session_id, None, *status, *at)
                .and_then(|()| match (status, perm) {
                    (Status::Perm, Some(p)) => {
                        let id = p.id.clone().unwrap_or_else(|| format!("t{at}"));
                        self.db
                            .insert_perm_event(session_id, &id, &p.tool, &p.summary, *at)
                    }
                    (Status::Perm, None) => Ok(()),
                    _ => self.db.end_perm_events(session_id, *at).map(drop),
                }),
            Event::SubAgentsChanged {
                session_id,
                sub_agents,
            } => sub_agents
                .iter()
                .try_for_each(|a| {
                    self.db
                        .transition_span(session_id, Some(a.id.as_str()), a.status, now)
                })
                .and_then(|()| {
                    let keep: Vec<&str> = sub_agents.iter().map(|a| a.id.as_str()).collect();
                    self.db
                        .close_other_sub_agent_spans(session_id, &keep, now)
                        .map(drop)
                }),
            Event::SubAgentStatus {
                session_id,
                sub_agent_id,
                status,
            } => self
                .db
                .transition_span(session_id, Some(sub_agent_id.as_str()), *status, now),
            Event::SessionClosed { session_id } | Event::SessionExited { session_id, .. } => self
                .db
                .close_session_spans(session_id, now)
                .and_then(|_| self.db.end_perm_events(session_id, now))
                .map(drop),
            _ => return,
        };
        if let Err(e) = result {
            tracing::warn!("history: {} not recorded: {e}", event.name());
        }
        self.prune_daily(now);
    }

    fn on_decision(&self, d: &PermDecisionRecord) {
        if let Err(e) = self.db.decide_perm_event(
            &d.session_id,
            d.prompt_id.as_deref(),
            d.decision,
            d.via,
            now_ms(),
        ) {
            tracing::warn!("history: decision not recorded: {e}");
        }
    }

    fn prune_daily(&self, now: u64) {
        let last = self.pruned_at.load(Ordering::Relaxed);
        if now.saturating_sub(last) >= DAY_MS
            && self
                .pruned_at
                .compare_exchange(last, now, Ordering::Relaxed, Ordering::Relaxed)
                .is_ok()
        {
            self.prune(now);
        }
    }

    fn prune(&self, now: u64) {
        self.pruned_at.store(now, Ordering::Relaxed);
        if let Err(e) = self.db.prune_history(now.saturating_sub(RETENTION_MS)) {
            tracing::warn!("history: pruning: {e}");
        }
    }
}
