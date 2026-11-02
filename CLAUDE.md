# Lotse: agent instructions

macOS terminal app (like iTerm2) with projects and agent awareness (like Orca). Rust daemon `lotsed` + Tauri 2 app (React 19, TypeScript, Tailwind 4, xterm.js). Plan: `ALPHA_PLAN.md`. UI spec: `spec/mock.html` (final UI) and `spec/research.html` (scope, security, sidebar/panel rules). All three are read-only.

## Rules
- **Terminal first.** The main view is the full-size terminal. Everything else stays out of the way.
- **No network listeners.** Unix socket only (`…/Lotse/lotsed.sock`, 0600, dir 0700, token first). No TCP in the app or daemon. The Vite dev server (loopback, dev only) is the one exception.
- **No telemetry, no update checks, no remote fetches at runtime.** No CDN assets.
- **Owns-paths.** Change only the paths your WP owns (`ALPHA_PLAN.md` › your WP) plus your own tests. Anything else (shared files, lockfiles, `shell/`, proto) goes in your report under "Requests".
- **Contracts are frozen after WP0:** `crates/lotse-proto` (+ `fixtures/`), `lotse-detect` `Adapter`/`Engine` API, `app/src/lib/client` (`DaemonClient`, `types.ts`). Changes need an orchestrator-approved proto bump (`PROTO_VERSION`).
- **Dependencies are pinned** (exact versions, lockfiles committed). Do not add or bump any.
- Tests never touch the real `~/.claude` or `~/Library/Application Support/Lotse`: use temp dirs. Never run a real agent in tests.
- Never print `.env` or secret values.
- Agents never commit, push or branch: the orchestrator does. Commits are Conventional Commits (`type(scope): summary`), **no `Co-Authored-By` trailers**.

## Commands
```sh
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cd app && npm ci
cd app && npm run lint && npm run typecheck && npx vitest run && npm run build
```
CI (`.github/workflows/ci.yml`, macos-14) runs exactly these.

## Run the app against the MockClient
```sh
cd app && npm run dev                 # http://127.0.0.1:1420, VITE_PORT or `-- --port N` to change
cd app && npm run dev -- --port 1421  # parallel agents: use the port you were given, never 1420 by accident
cd app && npm run tauri dev           # Tauri window (WP8 wires the real daemon client)
```
`createClient()` (`app/src/lib/client`) returns a `MockClient` seeded from `crates/lotse-proto/fixtures/snapshot_mock.json` (= `spec/mock.html` data: 5 projects, 7 sessions, sub-agents, every status) with scripted status changes (`DEFAULT_SCRIPT`, ~40 s: a5 done, a2 run→fail, a1 sub-agent done, a1 perm, auto-allow). Tests: `new MockClient({ script: false })`, advance with `client.tick(ms)`. Get the client in components with `useClient()`.

## Layout
- `crates/lotse-proto`: frames, messages, ids, `Status`, `rollup`, fixtures. `lotse-detect`: `Adapter`, `Engine`; modules `engine claude osc7501 screen opencode` are pre-declared stubs. `lotsed`, `lotse-hook`, `lotse-git`: stubs.
- `app/src/shell`: window layout, owns the named slots (`data-slot`). Each WP owns one folder with an exported component the shell already mounts: `sidebar/Sidebar`, `statusbar/StatusBar`, `panes/Panes` + `panes/TabStrip`, `terminal/TerminalView`, `sheets/Sheets`, `settings/SettingsPage`, `palette/Palette`. Keep those export names. Replace the placeholder body, do not edit `shell/`.
- `app/src/lib/ui/commands.ts`: UI command bus (`emitUiCommand` / `onUiCommand`): `openSettings openPalette openNewSession openAddProject toggleSidebar`. The shell and sidebar raise them; WP5 listens.
- `app/src/lib/ui/selection.ts`: the shared current session (`selectSession`, `useSelectedSessionId`). Sidebar and palette write it, panes/terminal/status bar read it. WP3 sets the initial selection.
- `DaemonClient.onConnection`: `connected | disconnected | reconnected`; on `reconnected` refetch `getSnapshot()` and reattach terminals.
- `npm run tauri dev` always uses port 1420 (`devUrl` in `tauri.conf.json`); do not run it in parallel with another dev server on 1420.
- `app/src/theme`: tokens from the mock (`--win --side --line --txt --dim --term --tdim --sel --blue …`), light/dark/auto via the `dark` class on `<html>`. Tailwind utilities: `bg-win bg-side bg-term bg-line bg-sel text-txt text-dim text-tdim text-blue text-orange text-red text-green`. Match `spec/mock.html` pixel for pixel; check against it in a browser.

## Conventions
- Rust 1.95, edition 2024, `rustfmt` defaults, no warnings. TypeScript strict, ESLint clean, `import type` for types.
- UI: Tailwind, light and dark both. Status names: `run perm fail done idle unknown`; roll-up perm > fail > run > done > idle.
- Parity gate before merge: compare your UI with `spec/mock.html` side by side (1440×900 and 1280×800, light and dark); screenshots in `parity/<wp>/`.
