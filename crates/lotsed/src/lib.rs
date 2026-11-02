//! `lotsed`: owns PTYs and state, serves the Unix socket (WP1).
//!
//! Alpha rules: Unix socket only (mode 0600 in a 0700 directory), token first, no TCP listener,
//! no telemetry. Sessions live in the daemon, so the app can quit and come back.
//!
//! - [`lifecycle`]: pidfile lock, listener, detached start, graceful shutdown
//! - [`server`]: handshake and per-client request loop
//! - [`state`]: registry (projects, sessions, settings) and the request dispatcher
//! - [`session`]: one PTY (threads, ring buffer, attached clients)
//! - [`launch`]: launch commands, PATH lookup, version detection, login environment
//! - [`db`]: SQLite schema and migrations

#![deny(unsafe_code)]

pub mod config;
pub mod db;
pub mod git;
pub mod hub;
pub mod launch;
pub mod lifecycle;
pub mod ring;
pub mod secure;
pub mod server;
pub mod session;
pub mod state;
pub mod title;
