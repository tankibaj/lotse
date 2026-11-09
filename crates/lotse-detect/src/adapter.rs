//! The adapter contract. Frozen after WP0.
//!
//! The daemon turns raw signals into an [`Observation`] and hands it to the [`crate::Engine`], which
//! offers it to the registered adapters. An adapter answers with zero or more [`Detection`]s; the
//! engine merges them by source precedence ([`DetectSource`]: mod > hook > osc7501 > screen > process).

use lotse_proto::{AgentKind, DetectSource, PermRequest, Status, SubAgent, UnixMs};

/// One raw signal about a session.
#[derive(Debug, Clone, PartialEq)]
pub enum Observation {
    /// Agent hook payload forwarded by `lotse-hook` (`Request::HookEvent`).
    Hook(HookObservation),
    /// OSC 7501 escape sequence found in PTY output.
    Osc(OscObservation),
    /// Rendered screen text from the daemon's VT model, for manifest matching.
    Screen(ScreenObservation),
    /// Foreground process of the PTY changed or exited.
    Process(ProcessObservation),
    /// Clock tick, for timeouts (e.g. "screen unchanged for N s → idle").
    Tick { now: UnixMs },
}

#[derive(Debug, Clone, PartialEq)]
pub struct HookObservation {
    pub agent: AgentKind,
    pub payload: serde_json::Value,
    pub at: UnixMs,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OscObservation {
    /// Payload after `ESC ] 7501 ;` up to the terminator.
    pub payload: String,
    pub at: UnixMs,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScreenObservation {
    /// Visible rows, top to bottom, trailing blanks trimmed.
    pub rows: Vec<String>,
    pub at: UnixMs,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProcessObservation {
    /// Executable name of the foreground process, if any (`None`: PTY child exited).
    pub foreground: Option<String>,
    pub exit_code: Option<i32>,
    pub at: UnixMs,
}

/// What an adapter concluded from an observation. `None` fields mean "no opinion".
#[derive(Debug, Clone, PartialEq)]
pub struct Detection {
    pub source: DetectSource,
    pub status: Option<Status>,
    /// Full replacement list of sub-agents.
    pub sub_agents: Option<Vec<SubAgent>>,
    /// Set together with `Status::Perm`.
    pub perm: Option<PermRequest>,
    pub at: UnixMs,
}

/// A detection source for one agent kind. One instance per session.
pub trait Adapter: Send {
    /// Stable name for logs, e.g. `claude-hooks`.
    fn name(&self) -> &'static str;

    /// The agent this adapter understands.
    fn agent(&self) -> AgentKind;

    /// The source its detections carry.
    fn source(&self) -> DetectSource;

    /// Feed one observation; return what changed (empty when the observation is not for this adapter).
    fn observe(&mut self, obs: &Observation) -> Vec<Detection>;

    /// Several observations that arrived together (the OSC reports of one output chunk). Same
    /// result as feeding them one by one, except that an adapter may report only the state it ends
    /// in, so a flood of reports costs one detection, not one per report.
    fn observe_batch(&mut self, batch: &[Observation]) -> Vec<Detection> {
        batch.iter().flat_map(|obs| self.observe(obs)).collect()
    }

    /// Asked on every clock tick: `true` when the adapter's signal is gone for good (the mod's
    /// heartbeat stopped). The engine then forgets everything this source said.
    fn expired(&mut self, _now: UnixMs) -> bool {
        false
    }

    /// The daemon's price table, for adapters that show a cost (the mod's sub-agent rows).
    fn set_pricing(&mut self, _pricing: crate::claude_mod::Pricing) {}
}
