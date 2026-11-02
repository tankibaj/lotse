//! `lotse-hook`: the tiny CLI that agent hooks call. It forwards the hook JSON from stdin to the
//! `lotsed` socket ([`forward`]) and never blocks or fails the agent. The same binary installs and
//! removes the Claude Code hooks ([`claude_settings`]).

pub mod claude_settings;
pub mod forward;
pub mod json;
