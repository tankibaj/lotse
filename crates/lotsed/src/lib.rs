//! `lotsed`: owns PTYs and state, serves the Unix socket (WP1).
//!
//! Alpha rules: Unix socket only (mode 0600 in a 0700 directory), token first, no TCP listener,
//! no telemetry. Sessions live in the daemon, so the app can quit and come back.
//!
//! - [`lifecycle`]: pidfile lock, listener, detached start, graceful shutdown
//! - [`server`]: handshake and per-client request loop
//! - [`state`]: registry (projects, sessions, settings) and the request dispatcher
//! - [`session`]: one PTY (threads, ring buffer, attached clients)
//! - [`detect_glue`]: hooks, OSC 7501, VT screen and process signals into the status engine (WP8)
//! - [`git_glue`]: `lotse-git` wiring: git line, worktrees (WP8)
//! - [`agent_env`]: agent-host variables dropped from panes, status plugins injected per pane
//! - [`child_policy`] / [`child_sessions`]: what an agent's `lotse-hook mcp` connection may ask
//!   (allowlist, limits) and the approval cards of the sessions it starts (proto 5)
//! - [`perm_answer`]: option list on a permission prompt → the keys that answer it
//! - [`launch`]: launch commands, PATH lookup, version detection, login environment
//! - [`db`]: SQLite schema and migrations
//! - [`transcript`]: agent transcript files (path validation, last turns)
//! - [`usage`]: today's tokens and cost from the Claude Code transcripts
//!
//! Beta stubs (proto 4), one per work package. The requests of [`review`], [`files`],
//! [`session_usage`], [`export`], [`session_info`] and [`mcp`] are routed to their `handle` and
//! answer `unavailable` until the package lands; [`pr`], [`history`], [`transcript_sources`],
//! [`redact`] and [`quiet`] are empty.

#![deny(unsafe_code)]

pub mod agent_env;
pub mod autoallow;
pub mod child_policy;
pub mod child_sessions;
pub mod config;
pub mod db;
pub mod detect_glue;
pub mod export;
pub mod files;
pub mod git;
pub mod git_glue;
pub mod history;
pub mod hub;
pub mod launch;
pub mod lifecycle;
pub mod mcp;
pub mod mod_source;
pub mod perm_answer;
pub mod pr;
pub mod quiet;
pub mod redact;
pub mod review;
pub mod ring;
pub mod secure;
pub mod server;
pub mod session;
pub mod session_info;
pub mod session_usage;
pub mod state;
pub mod task_watch;
pub mod title;
pub mod transcript;
pub mod transcript_sources;
pub mod two_way;
pub mod usage;
