//! `lotse-hook`: the tiny CLI that agent hooks call. It forwards the hook JSON from stdin to the
//! `lotsed` socket ([`forward`]) and never blocks or fails the agent. The same binary installs and
//! removes the Claude Code hooks ([`claude_settings`]), and builds the per-pane plugins the daemon
//! injects ([`claude_plugin`], [`opencode_plugin::overlay`], [`codex_home::mirror`]).

pub mod claude_plugin;
pub mod claude_settings;
pub mod codex_home;
pub mod forward;
pub mod json;
pub mod mcp;
pub mod mcp_inject;
pub mod mcp_link;
pub mod opencode_plugin;
