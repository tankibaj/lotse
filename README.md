# Lotse

A macOS terminal app with projects and agent awareness. The main view is the full-size agent CLI (Claude Code, Codex, OpenCode or a shell); the sidebar shows every session's status (run, perm, fail, done, idle) and sub-agents.

Status: Alpha in progress, see `ALPHA_PLAN.md` and `STATUS.md`. Apple Silicon, macOS only. No telemetry, no network listeners.

## Architecture
- `lotsed` (Rust daemon) owns the PTYs and state and listens on a Unix socket (`~/Library/Application Support/Lotse/lotsed.sock`).
- The app (Tauri 2, React, Tailwind, xterm.js) is a client; quitting it does not kill sessions.
- `lotse-proto` defines the wire protocol (`PROTO_VERSION = 1`); JSON fixtures in `crates/lotse-proto/fixtures` are the shared contract with the TypeScript `DaemonClient`.

## Develop
Requires Rust 1.95 (`rust-toolchain.toml`) and Node 22.

```sh
cargo test --workspace
cd app && npm ci
cd app && npm run dev          # UI with the MockClient: http://127.0.0.1:1420 (VITE_PORT to change)
cd app && npm run tauri dev    # Tauri window
cd app && npm run lint && npm run typecheck && npx vitest run && npm run build
```

Agents working on the repo: read `CLAUDE.md`.
