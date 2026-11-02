//! Claude Code hook adapter (WP2). Stub: WP2 adds `pub struct ClaudeAdapter` implementing
//! [`crate::Adapter`] with `DetectSource::Hook`, including sub-agent (Task) tracking.

use crate::Adapter;
use lotse_proto::{AgentKind, SessionId};

/// Called by `Engine::with_default_adapters`. WP2 returns the adapter for `AgentKind::ClaudeCode`.
pub fn default_adapter(_session: &SessionId, _agent: AgentKind) -> Option<Box<dyn Adapter>> {
    None
}
