//! Screen-scrape adapter driven by bundled TOML manifests, herdr style (WP6). Stub: WP6 adds the
//! manifest loader and an adapter with `DetectSource::Screen`. No remote manifest fetch, ever.

use crate::Adapter;
use lotse_proto::{AgentKind, SessionId};

/// Called by `Engine::with_default_adapters`. WP6 returns the manifest-driven adapter for `agent`.
pub fn default_adapter(_session: &SessionId, _agent: AgentKind) -> Option<Box<dyn Adapter>> {
    None
}
