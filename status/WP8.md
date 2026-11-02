# WP8 · Integration

| Field | Value |
|---|---|
| Agent / run | claude CLI (harnex, headless) · wp8-integration |
| Worktree / branch | ~/Repos/incubator/lotse-wt/wp8-integration · feature/wp8-integration |
| Started | 2026-11-02 04:18 |
| ETA (first) | 08:30 (≈ 4 h) |
| ETA (current) | done 05:00 |
| Phase | done |
| Progress | 13 / 13 tasks |
| Last update | 2026-11-02 05:00 |

## Tasks
- [x] 1 daemon ↔ detection glue (hook, OSC, VT screen, process, tick) + events
- [x] 2 hook env contract + `message.rs` doc
- [x] 3 git wiring (`git_info`, worktree sessions, `git_info_changed`)
- [x] 4 Tauri bridge + `RealClient` behind a flag
- [x] 5 notifications + Dock badge
- [x] 6 host bridge (scan/clone/create, dialog capability; drag-drop already in `hostBridge.ts`)
- [x] 7 links / Reveal in Finder (own commands, no new dependency)
- [x] 8 UI commands + sidebar menu items
- [x] 9 state dedupe
- [x] 10 settings round-trip (theme, hook/plugin installer buttons)
- [x] 11 retry semantics
- [x] 12 `window.__lotse` untouched (still merged by `lib/state` and `sheets`)
- [x] 13 WP6 glue + `lotse-hook integration opencode`
- [x] e2e, full CI set, tauri build, parity, review

## Log (newest last, `HH:MM phase — note`)
- 04:18 plan — started; read WP1–WP7 requests.
- 04:25 implement — detect glue (VT fed from the tick, off the reader), OSC scanner inline.
- 04:31 test — flood test failed with the VT parse on the reader → moved to the tick; worktree-on-non-git is now `invalid_request` (test updated).
- 04:36 implement — Tauri bridge; 04:39 vitest 322 green.
- 04:50 test — bridge refactored behind `Sink`; `tests/bridge.rs` runs the bridge against the real `lotsed` (3 tests). App started with `--isolated-data-dir`: it started the daemon through the real webview IPC.
- 04:52 parity — Playwright 48/48 (all e2e) + 12/12 (sheets, :1424); screenshots in `parity/wp8/`; label fixes "Open in Split Right", "Stop Session".
- 04:54 review — harnex:reviewer (sonnet): CHANGES, 3 MUST-FIX + 5 NICE. Fixed: stale PTY backlog, second permission request not notifying, worktree left behind on a failed launch, connect/reconnect double link, partial-frame on write timeout. Not fixable here: parity PNG churn (see Requests), sound-only (needs proto).
- 05:00 done — final CI set green (numbers below).

## Handoffs and bounce-backs
| Time | Type | From → To | Reason | Resolved |
|---|---|---|---|---|
| 04:54 | bounce-back | harnex:reviewer → WP8 | CHANGES: 3 MUST-FIX, 5 NICE-TO-HAVE | 04:58 fixed 1 (backlog) + 4 NICE; 1 reported (PNG churn), 1 documented (sound-only) |

## Blockers / requests for shared files
See Final report › Requests.

## Final report

**Files:** Rust 24 (lotsed: 2 new src `detect_glue.rs` `git_glue.rs`, 5 changed, 1 new test `detection.rs`, 1 test updated; lotse-hook: 1 new src `opencode_plugin.rs`, 3 changed, 1 new test `env_contract.rs`; lotse-proto: doc comment only; app/src-tauri: new `daemon.rs` `host.rs` `tests/bridge.rs`, changed `lib.rs` `Cargo.toml` `capabilities/default.json`) · Web 19 changed + 6 new (`lib/client/{realClient,realNative}.ts` + test, `settings/integration.ts` + test, `e2e/wp8-integration.spec.ts`, `e2e/playwright.all.config.ts`) · `parity/wp8/` 24 PNG · `Cargo.lock` (see Requests).

**Done**
- **Daemon ↔ detection:** `Detector` per session feeds `Engine` from hooks (`HookEvent`), OSC 7501 (scanner inline; queries answered with `QUERY_REPLY`), the `alacritty_terminal` VT model (fed from a 100 ms tick, off the reader thread; sampled after ~100 ms quiet, at most every 500 ms while streaming, never inside `?2026`, a stuck sync is flushed after 150 ms), foreground process (tcgetpgrp + `ps`, switches the engine agent when `claude`/`codex`/`opencode` takes the foreground and back to the shell), child exit and a 1 s Tick. Emits `status_changed`, `sub_agents_changed`, `sub_agent_status`, `git_info_changed`, sets `unread` on a finished turn, and emits `Event::Notification` with global toggles and per-project override applied (perm / fail / done / sub-agent done; shell sessions never notify).
- **Hook contract:** `lotse-hook` reads `LOTSE_SESSION_ID`, `LOTSE_SOCKET`, `LOTSE_TOKEN_FILE` (token only from the file); data-dir fallback kept; exit-0-fast untouched. `LOTSE_HOOK_BIN` exported to panes. `lotse-hook integration opencode install|uninstall|status` (marker, backup, idempotent, never overwrites a foreign file). `HookEvent` doc comment fixed.
- **Git:** `lotse_git::git_info` for session cards and `get_git_info` (a session's own worktree), refreshed every 5 s (once per folder); worktree sessions via `create_worktree` (removed again when the launch fails).
- **Tauri bridge:** one Unix-socket link (`daemon.rs`), requests, events, PTY channel, raw keystrokes, daemon start (`lotsed --detach`), reconnect with restart, notification settings re-synced on (re)connect, `Sink` abstraction so it is tested against the real daemon. `RealClient` (`createClient()`: app → real, browser → mock, `VITE_LOTSE_CLIENT=mock|real` forces). Host commands: `scan_repos`, `clone_repo` (`--`, `GIT_ALLOW_PROTOCOL`), `create_project_dir`, `open_url` (http/https), `reveal_in_finder`, `integration_*`; `dialog:allow-open`; Dock badge + focused-session reporting.
- **UI:** `openProjectStyle`, `splitSession`, `openNewSession(projectId)`; menu items Change Icon & Color…, Reveal in Finder (app), Open in Split Right; cards carry `data-session-id`; `useSessions` is a view of `useWorkspace`; theme: daemon wins; Install/Remove hooks and plugin buttons; Retry on a live process shows the daemon's message.

**Tests (my runs, final state)**
- `cargo fmt --all --check` clean · `cargo clippy --workspace --all-targets -- -D warnings` clean · `cargo test --workspace`: **261 passed, 0 failed, 1 ignored** (lotsed 42 unit + detection 6 + lifecycle 6 + security 9 + sessions 18 + settings 6; lotse-app 24 + bridge 3; lotse-hook 34 + cli 4 + env_contract 3; lotse-detect 53 + 3 + 3 + 5; lotse-git 20; lotse-proto 22).
- `npm run lint` clean · `npm run typecheck` clean · `npx vitest run`: **25 files, 322 tests passed** · `npm run build` ok · `npx tauri build --debug --no-bundle` ok.
- Playwright: all `e2e/` (WP3 + WP4 + WP8, port 1421, via `e2e/playwright.all.config.ts`): **48/48**; `src/sheets/e2e/playwright.chrome.config.ts` (1424): **12/12**.
- Done-when scripted e2e: `app/src-tauri/tests/bridge.rs::app_starts_the_daemon_events_flow_and_a_relaunch_finds_the_session_with_scrollback` (start daemon from the app, add temp project, open shell session, PTY echo, real `lotse-hook` binary → `status_changed` run reaches the sink, quit, relaunch: session + status + scrollback replay) and `lotsed/tests/detection.rs::app_relaunch_finds_the_session_with_its_scrollback`; plus `a_dead_daemon_is_noticed_restarted_and_reconnected`.
- Manual smoke: `cargo run -p lotse-app --features tauri/custom-protocol -- --isolated-data-dir <dir>`: the window started the daemon through the real WKWebView IPC (socket, token, db created) and kept a connection; killed afterwards.
- Known flake (one vitest test, 2× in 20 runs): not seen in 6 full runs.
- harnex:reviewer (sonnet): CHANGES → fixed as in the log. No validator run, no UI/UX reviewer (budget): test numbers above are mine, not independent.

**Parity** (mock = `spec/mock.html` via file:// in Chrome; app = web build + MockClient; 1440×900 and 1280×800, light and dark; `parity/wp8/{mock,app}-{project-menu,session-menu,split}-{light,dark}-{1440,1280}.png`)
| Feature | Mock | App | Result |
|---|---|---|---|
| Project menu items and order | New Session…, Notifications ›, Change Icon & Color…, Reveal in Finder, Copy Path, Rename…, Remove | same | match |
| Change Icon & Color… | opens the colour/icon popover | opens it under the row (UI command) | match |
| New Session… from the project menu | preselects the project | same | match |
| Reveal in Finder | enabled, ⌥⌘R hint | app: enabled (Finder); browser: disabled "App only"; hint not shown | accepted (browser has no Finder; hints for unbound keys left out, WP3) |
| Session menu | Open, Open in Split Right (⌥-click), Show Transcript, Show Changes, Stop Session | same labels; Transcript/Changes disabled "Beta" | match (labels fixed this run: were "Split Right", "Stop") |
| Split Right / ⌥-click / drag | 2nd pane on the right, focus moves | same, up to 4 panes | match |
| Right pane content | same screen in both panes (mock fixture) | each session's own screen | accepted (mock is static) |
| First menu item highlighted on open | none | first item focused (keyboard access) | accepted (a11y, WP3) |
| Status run/perm/fail/done/idle, sub-agents, git line, unread, notification kinds (real daemon) | mock states | `detection.rs` reproduces run → perm → run → done, sub-agents, unread, git ↑/±/worktree, per-project mute | match |
| Notification defaults, mute, custom | perm banner+sound, done banner, sub-agent off | `detect_glue::notification` tests | match; gap: "banner off, sound on" sends nothing (proto has no banner flag) |
| Settings › Agents integrations | "hooks installed" label | label + Install/Remove button (app only) | match + addition |
| Window frame offset (mock 32 px) | gradient frame | full window | WP0 shell, not WP8 |

**Requests (shared files / orchestrator)**
1. **Before committing: `git checkout -- parity/wp3 parity/wp4`.** Running the existing WP3/WP4 Playwright specs rewrote 24 of their committed PNGs (they save screenshots); I cannot restore files (no checkout allowed). Only `parity/wp8/` is mine.
2. `Cargo.lock` changed: adding `tempfile` as a dev-dependency of `lotse-app` (declared in the workspace already) added a lockfile edge. No new crate versions, no `package.json` change.
3. **Packaging:** the app finds `lotsed` and `lotse-hook` next to its own executable (`LOTSE_DAEMON_BIN` / `LOTSE_HOOK_BIN` override). `tauri build --debug --no-bundle` leaves them in `target/debug` only after `cargo build --workspace`. A release bundle needs both binaries in `Contents/MacOS` (Tauri `externalBin` or a build step): `tauri.conf.json` is not mine.
4. **Proto:** `Event::Notification` needs a `banner: bool` (or kind of delivery) to support "sound only"; until then the daemon sends nothing for banner-off. Proto change = bump, not done.
5. `ALPHA_PLAN.md` says "tauri-plugin-opener": not added (no network for a new dependency, and `open`/`open -R` with validated arguments is smaller); say if you want the plugin anyway.
6. `app/e2e/playwright.all.config.ts` (new, not CI) points all e2e specs at 1421 so the WP4 specs (default 1422) run without env vars.

**Open questions / defaults taken**
- Retry on a live process: refused, daemon message shown in the sidebar (default 11). Exited → re-runs the last command (WP1).
- `unread` is set when a turn finishes (Done/Fail from the engine); the selected session is marked read by the store as before. A process exit does not set it.
- Settings › Appearance: fonts were already applied at start by `SettingsPage`; the terminal reads the same daemon settings, so no CSS-variable plumbing was added.
- Shell sessions never notify (only sessions running an agent).
- Items skipped: none. Item 6 drag-drop was already implemented by WP5 (`tauriDrops`); only the capability/commands were missing.

**Unverified**
- **Manual: a real Claude Code session run → perm → run → done in the sidebar (not run: headless, no real agent).** Steps for Naim: `cargo build --workspace && npm run tauri dev` (or the built `target/debug/lotse-app`); Settings › Agents › Claude Code › "Install hooks" (writes `~/.claude/settings.json`, backup next to it); add a project; New session › Claude Code; give it a task that needs a tool permission. Expect the card: run (spinner) → perm (Allow/Always/Deny, Dock badge 1, banner "praxis-web · Claude Code / Needs permission") → run → done (unread dot, banner "Finished").
- Tauri API behaviour beyond a compile and one smoke start: `Channel::send(Raw)` to JS (`ArrayBuffer`), raw `invoke` bodies with the `x-lotse-session` header (sync command, call order), `onDragDropEvent`, capability `dialog:allow-open`. Docs: https://v2.tauri.app/develop/calling-rust/ . I could not see the window, so the sidebar/terminal in the real app, the Dock badge and the banner/sound are not visually confirmed.
- Real foreground process names (`ps -o comm=`) of `claude`, `codex`, `opencode` (node-based installs show `node`); hook event names and payload fields (WP2's list), OSC 7501 emitters, Codex/OpenCode screen text: all as in the WP2/WP6 reports.
- `lotse-hook integration opencode`: that OpenCode loads plugins from `~/.config/opencode/plugins/` (https://opencode.ai/docs/plugins/).
- Performance with many sessions (100 ms tick takes each session's lock; VT parse capped at 256 KiB per tick); only the 200 MB flood test and 7-session runs were exercised.
