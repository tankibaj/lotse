# Lotse: agent instructions

macOS terminal app with projects and agent awareness. Rust daemon `lotsed` + Tauri 2 app (React 19, TypeScript, Tailwind 4, xterm.js). Version `0.1.6` (Beta). Read `README.md` for what the app does and how status works.

## Rules
- **Terminal first.** The main view is the full-size terminal. Everything else stays out of the way.
- **No network listeners.** Unix socket only (`…/Lotse/lotsed.sock`, 0600, dir 0700, token first). No TCP in the app or daemon. The Vite dev server (loopback, dev only) is the one exception.
- **No telemetry, no update checks, no remote fetches at runtime.** No CDN assets.
- **Contracts are frozen:** `crates/lotse-proto` (+ `fixtures/`), the `lotse-detect` `Adapter`/`Engine` API, and `app/src/lib/client` (`DaemonClient`, `types.ts`). A change needs a deliberate `PROTO_VERSION` bump and updated fixtures.
- **Dependencies are pinned** (exact versions, lockfiles committed). Do not add or bump any without being asked.
- **Tests never touch** the real `~/.claude`, `~/.codex` or `~/Library/Application Support/Lotse`: use temp dirs. Never run a real agent in tests.
- **Public repo hygiene:** no personal names, employer or client names, real hostnames or absolute home paths in code, fixtures, tests, docs or screenshots. Use placeholders (`web-app`, `api`, `$HOME`). No secrets or real tokens, not even in test data.
- Never print `.env` or secret values.
- Commits are Conventional Commits (`type(scope): summary`), no `Co-Authored-By` trailers. Do not commit, push or branch unless asked.

## Commands
```sh
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cd app && npm ci
cd app && npm run lint && npm run typecheck && npx vitest run && npm run build
```
CI (`.github/workflows/ci.yml`, macos-26) runs these.

App build and audits (not in CI):
```sh
sh scripts/build-app.sh               # release lotsed + lotse-hook + .app (target/release/bundle/macos/Lotse.app), ad-hoc sign, smoke test
node scripts/licenses.mjs             # licence summary of runtime deps (THIRD_PARTY_NOTICES.md); flags MPL, exit 1 only on GPL/AGPL
cargo audit; cd app && npm audit --omit=dev
```

## Run the app
```sh
cd app && npm run dev                 # UI with the MockClient: http://127.0.0.1:1420 (VITE_PORT or `-- --port N`)
cd app && npm run dev -- --port 1421  # parallel agents: use your own port, never 1420 by accident
cd app && npm run tauri dev           # Tauri window with the real daemon (always port 1420: do not run it in parallel with another dev server on 1420)
```
`createClient()` (`app/src/lib/client`) returns a `MockClient` seeded from `crates/lotse-proto/fixtures/snapshot_mock.json` with scripted status changes. Tests: `new MockClient({ script: false })`, advance with `client.tick(ms)`. Get the client in components with `useClient()`.

## Layout
- `crates/lotse-proto`: frames, messages, ids, `Status`, roll-up, fixtures (the shared contract with the TypeScript client). `lotse-detect`: agent status from screen and OSC signals (`Adapter`, `Engine`). `lotsed`: the daemon (PTYs, sessions, permissions, auto-allow, git glue, PR status, child sessions). `lotse-hook`: the CLI the integrations call (Claude plugin, Codex home, OpenCode plugin, MCP). `lotse-git`: branch, worktree, diff and review data.
- `app/src/shell`: window layout and the named slots (`data-slot`). Feature folders mounted by the shell: `sidebar`, `statusbar`, `panes` (+ tab strip), `terminal`, `sheets`, `settings`, `palette`, `panels`, `review`, `editor`, `composer`. Keep their exported component names.
- `app/src/lib/ui/commands.ts`: UI command bus (`emitUiCommand` / `onUiCommand`). `app/src/lib/ui/selection.ts`: the shared current session (`selectSession`, `useSelectedSessionId`).
- `DaemonClient.onConnection`: `connected | disconnected | reconnected`; on `reconnected` refetch `getSnapshot()` and reattach terminals.
- `app/src/theme`: tokens (`--win --side --line --txt --dim --term --tdim --sel --blue …`), light/dark/auto via the `dark` class on `<html>`. Tailwind utilities: `bg-win bg-side bg-term bg-line bg-sel text-txt text-dim text-tdim text-blue text-orange text-red text-green`.
- `integrations/`: agent integrations and manifests (Apache-2.0 files derived from herdr keep their notices; see `THIRD_PARTY_NOTICES.md`).
- `spec/mock.html`: the UI reference mock. Compare UI changes with it side by side (1440×900 and 1280×800, light and dark).

## Conventions
- Rust 1.95, edition 2024, `rustfmt` defaults, no warnings. TypeScript strict, ESLint clean, `import type` for types.
- UI: Tailwind, light and dark both. Status names: `run perm fail done idle unknown`; roll-up perm > fail > run > done > idle.
- Status integrations are per pane (Claude plugin via `--plugin-dir`, mirrored `CODEX_HOME`, OpenCode plugin). Lotse must not write global agent config; it only offers cleanup of old global hooks.
- Keep `README.md` shortcuts and status-source sections in step with `app/src/lib/state/shortcuts.ts`, `app/src/shell/Shell.tsx` and `crates/lotse-hook`.
