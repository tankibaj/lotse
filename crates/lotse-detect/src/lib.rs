//! Lotse status engine and adapters.
//!
//! Detection order (strongest first): Claude Code / Codex hooks → OSC 7501 → screen scrape (TOML manifests)
//! → process-group identity. `adapter` holds the frozen contract (the `Adapter` trait and the
//! observation/detection types); the other modules are stubs that WP2 and WP6 fill in without
//! touching this file.

pub mod adapter;
pub mod claude; // WP2
pub mod claude_mod; // Beta M2
pub mod codex;
pub mod engine; // WP2
pub mod opencode; // WP6
pub mod osc7501; // WP6
pub mod screen; // WP6

pub use adapter::{
    Adapter, Detection, HookObservation, Observation, OscObservation, ProcessObservation,
    ScreenObservation,
};
pub use engine::{Engine, StatusUpdate};
