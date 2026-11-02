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
//! - [`launch`]: launch commands, PATH lookup, version detection, login environment
//! - [`db`]: SQLite schema and migrations

#![deny(unsafe_code)]

pub mod agent_env;
pub mod config;
pub mod db;
pub mod detect_glue;
pub mod git;
pub mod git_glue;
pub mod hub;
pub mod launch;
pub mod lifecycle;
pub mod ring;
pub mod secure;
pub mod server;
pub mod session;
pub mod state;
pub mod title;
