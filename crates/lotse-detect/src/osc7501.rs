//! OSC 7501 adapter (WP6). Stub: WP6 adds the `ESC ] 7501 ;` parser and an adapter with
//! `DetectSource::Osc7501` (Codex and any agent that emits it).

use crate::Adapter;
use lotse_proto::{AgentKind, SessionId};

/// Called by `Engine::with_default_adapters`. WP6 returns the OSC adapter for agents that emit it.
pub fn default_adapter(_session: &SessionId, _agent: AgentKind) -> Option<Box<dyn Adapter>> {
    None
}
