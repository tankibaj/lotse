//! OpenCode adapter (WP6). Stub: WP6 adds the adapter for the OpenCode plugin's events.

use crate::Adapter;
use lotse_proto::{AgentKind, SessionId};

/// Called by `Engine::with_default_adapters`. WP6 returns the adapter for `AgentKind::OpenCode`.
pub fn default_adapter(_session: &SessionId, _agent: AgentKind) -> Option<Box<dyn Adapter>> {
    None
}
