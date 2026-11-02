//! Status engine (WP2 implements the bodies). Public API is frozen.
//!
//! One `Engine` per session: owns the registered adapters, merges their detections by source
//! precedence, and reports a [`StatusUpdate`] only when status, sub-agents or the permission
//! request actually change.

use crate::adapter::{Adapter, Observation};
use lotse_proto::{AgentKind, DetectSource, PermRequest, SessionId, Status, SubAgent, UnixMs};

/// A merged change, ready to become `Event::StatusChanged` / `Event::SubAgentsChanged`.
#[derive(Debug, Clone, PartialEq)]
pub struct StatusUpdate {
    pub status: Status,
    pub source: DetectSource,
    pub perm: Option<PermRequest>,
    /// `Some` only when the sub-agent list changed.
    pub sub_agents: Option<Vec<SubAgent>>,
    /// Set while `status == Run`.
    pub running_since: Option<UnixMs>,
    pub at: UnixMs,
}

pub struct Engine {
    session_id: SessionId,
    agent: AgentKind,
    adapters: Vec<Box<dyn Adapter>>,
}

impl Engine {
    /// Engine with no adapters; status stays `Unknown`.
    pub fn new(session_id: SessionId, agent: AgentKind) -> Self {
        let _ = (&session_id, &agent);
        todo!("WP2")
    }

    /// Engine with the bundled adapters for `agent`: WP2 implements this by registering whatever
    /// `claude::`, `osc7501::`, `screen::` and `opencode::default_adapter(&session_id, agent)`
    /// return (each stub module owns its constructor, so WP6 never edits this file).
    pub fn with_default_adapters(session_id: SessionId, agent: AgentKind) -> Self {
        let _ = (&session_id, &agent);
        todo!("WP2")
    }

    pub fn session_id(&self) -> &SessionId {
        &self.session_id
    }

    pub fn agent(&self) -> AgentKind {
        self.agent
    }

    pub fn register(&mut self, adapter: Box<dyn Adapter>) {
        let _ = (&mut self.adapters, adapter);
        todo!("WP2")
    }

    /// Offer an observation to every adapter and merge; `None` when nothing changed.
    pub fn observe(&mut self, obs: Observation) -> Option<StatusUpdate> {
        let _ = obs;
        todo!("WP2")
    }

    /// Current merged status.
    pub fn status(&self) -> Status {
        todo!("WP2")
    }

    /// Current sub-agents.
    pub fn sub_agents(&self) -> &[SubAgent] {
        todo!("WP2")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[ignore = "WP2: engine not implemented"]
    fn new_engine_starts_unknown() {
        let e = Engine::new(SessionId::new("a1"), AgentKind::ClaudeCode);
        assert_eq!(e.status(), Status::Unknown);
        assert!(e.sub_agents().is_empty());
    }

    #[test]
    #[ignore = "WP2: engine not implemented"]
    fn stronger_source_wins_over_weaker() {
        // hook says perm, a later screen detection says run: status stays perm until the hook clears it.
        todo!("WP2")
    }
}
