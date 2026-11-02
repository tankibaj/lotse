# WP0 · Foundation and contracts

| Field | Value |
|---|---|
| Agent / run | claude CLI (harnex, headless) · /Users/naim/Repos/incubator/lotse-wt/wp0-foundation |
| Worktree / branch | ~/Repos/incubator/lotse-wt/wp0-foundation · feature/wp0-foundation |
| Started | 2026-11-02 01:44 |
| ETA (first) | 03:15 (≈ 90 min) |
| ETA (current) | done 02:12 |
| Phase | done (uncommitted in worktree; orchestrator commits) |
| Progress | 9 / 9 tasks · 100 % |
| Last update | 2026-11-02 02:12 |

## Tasks
- [x] Cargo workspace + crates (proto, lotsed, detect, hook, git, src-tauri), deps pinned
- [x] lotse-proto: codec, messages, status, roll-up, fixtures + round-trip tests
- [x] lotse-detect: Adapter trait, engine API, stub modules
- [x] app: npm deps pinned, Vite/React/Tailwind/Tauri skeleton
- [x] app: theme tokens, shell slots, WP folders with placeholders
- [x] app: DaemonClient + MockClient (+ tests)
- [x] CLAUDE.md, README.md, CI
- [x] Run all CI commands
- [x] Parity screenshots + table, fresh review (1 round, fixes applied)

## Log (newest last, `HH:MM phase — note`)
- 01:44 plan — started (WP section is the approved plan; skipped planner), ETA 03:15
- 01:50 implement — read plan + mock; workspace + proto
- 01:58 test — cargo 17 proto tests, vitest 20, lint/typecheck/build green
- 02:00 parity — screenshots 1440×900 / 1280×800, light/dark, mock vs app; metrics identical
- 02:04 review — fresh `harnex:reviewer`: CHANGES (5 MUST-FIX, 6 NICE-TO-HAVE)
- 02:10 implement — fixed MUST-FIX 1-5 + NICE 6,7,8,10,11; full suite re-run green
- 02:12 done — final report below

## Handoffs and bounce-backs
| Time | Type | From → To | Reason | Resolved |
|---|---|---|---|---|
| 02:04 | bounce-back | reviewer → WP0 | CHANGES: drag capability, onConnection, selection contract, mock unread/sub-agent events, codec test gaps | yes, 02:10 |

## Blockers / requests for shared files
- none (no edits outside owned paths). See "Requests" in the final report.

## Final report
**Files (uncommitted, ~95 incl. 8 PNGs):** Rust workspace + crates 28 (proto 7 src + 5 fixtures, detect 8, git 2, hook 2, lotsed 2, root `Cargo.toml`/`Cargo.lock`/`rust-toolchain.toml`) · Tauri shell 11 (`app/src-tauri`: Cargo, build.rs, lib/main, conf, capability, 5 icons + `app-icon.png`) · app TS/CSS 45 (shell 3, WP folders 14, lib/client 7, lib/ui 4, lib/state 1, theme 4, tests 6, config 8) · CI 1 · docs 2 (`CLAUDE.md`, `README.md`) · `.gitignore` · parity PNGs 8.

**Done:** all WP0 scope items. `STATUS.md` untouched (orchestrator-owned).

**Tests (my runs; the orchestrator re-runs CI on `main`):**
- `cargo fmt --all --check` ✓ · `cargo clippy --workspace --all-targets -- -D warnings` ✓ (0 warnings)
- `cargo test --workspace`: 23 passed (lotse-proto 22, lotsed 1), 3 ignored (detect 2, git 1 stubs), 0 failed. Proto fixtures: 67 messages in 5 files, every one round-trips JSON ↔ `Message` ↔ frame bytes; fixtures cover all 23 request methods, 12 response kinds, 13 events (test fails if a variant has no fixture); `snapshot_mock.json` reproduces the mock (5 projects, 7 sessions, all 5 statuses, sub-agent run/done/perm, "No git", worktree vs main, perm card, unread, roll-up per project).
- `npm ci` ✓ · `npm run lint` ✓ · `npm run typecheck` ✓ · `npx vitest run`: 5 files, 24 passed · `npm run build` ✓ (245 kB JS, 11.5 kB CSS)
- `npx tauri build --debug --no-bundle` ✓ (Tauri config, capabilities, icons, CSP accepted; binary built)
- Review: 1 fresh `harnex:reviewer` round, 5 MUST-FIX fixed, 4 NICE-TO-HAVE fixed, 2 left (see Open).

**Parity (mock = `spec/mock.html` at `http://127.0.0.1:1421/@fs/…`, app = `npm run dev -- --port 1421`; screenshots `parity/wp0/{mock,app}-{1440x900,1280x800}-{light,dark}.png`; metrics via `getBoundingClientRect`/`getComputedStyle`, mock window offset by its 32 px page padding subtracted):**

| Feature | Mock | App | Match |
|---|---|---|---|
| Sidebar width / bg / border | 260 px, #F5F5F7, #E5E5EA | 260 px, same | match |
| Title row (dots, "Lotse", gear, hide btn) | h 44, title at (80,12) | same position/size, dots drawn only in browser | match |
| Search field | (12,44) 235×32, bg #E5E5EA | same | match |
| "PROJECTS +" header | y 92, h 26 | same | match |
| Tab row / main split | h 44, main x = 262 (260 + 2 px grip) | same | match |
| Terminal surface | #FFF / color #1D1D1F, mono 13 px | same | match |
| Status bar | h 28, 11 px, bg #F5F5F7 | same | match |
| Fonts | system stack, 13 px; mono `ui-monospace, SF Mono, Menlo` | same | match |
| Dark tokens | side #252527, line #38383A, win/term #1E1E1E, txt #F5F5F7, search #38383A, orange #FF9F0A | same | match |
| Light/dark/auto | `lotse-theme` key, `dark` class | same key/mechanism | match |
| Hide sidebar ⌃⌘S + ☰ show button | works | works (checked in browser) | match |
| Project colours | hash → purple, teal, yellow, sage, indigo | fixture/MockClient identical (verified via mock's `pstyle`) | match |
| Window frame (gradient bg, 32 px inset, rounded, shadow, fake dots) | page chrome of the mock | app fills viewport; native lights in Tauri (dots only in browser) | accepted deviation: the mock draws a window inside a page |
| Right-panel toolbar (Session/Changes/Files/Transcript/Usage icons) | shown | absent | accepted deviation: Beta, out of Alpha |
| Sidebar tree, tab content, terminal text, footer data ("▲ 2 need you", git, "6 sessions", tokens "412k · $4.18") | live | placeholders (`— need you`, `Today — tokens · —`) | gap by design: WP3/WP4; tokens `—` is an Alpha decision |
| Sheets, Settings, palette, banners | live | hidden placeholders, UI command bus ready | gap by design: WP5 |

**Frozen contracts (names only):**
- Proto `Message`: `hello hello_ok request response event`. `Request` (23): `ping get_snapshot list_projects add_project update_project remove_project list_sessions create_session rename_session close_session retry_session mark_read resolve_permission attach detach send_input resize get_settings update_settings detect_launch_commands get_git_info hook_event shutdown`. `Response` (12): `ok pong snapshot projects project sessions session attached settings launch_commands git_info error`. `Event` (13): `project_added project_updated project_removed session_created session_updated session_closed session_exited status_changed sub_agents_changed sub_agent_status git_info_changed settings_changed notification`. Frames: `0x01` control JSON, `0x02` PTY (`u16 id_len | id | bytes`), u32 BE length, `MAX_FRAME_LEN` 16 MiB, `MAX_HELLO_LEN` 64 KiB, `PROTO_VERSION = 1`; `encode decode decode_max read_frame read_frame_max write_frame`. Types: `Project Session SubAgent GitInfo PermRequest PermDecision NewSession NewProject ProjectPatch WorktreeSpec LaunchCommandInfo LaunchCommands Settings NotificationSettings EventPrefs AppearanceSettings Theme NotificationMode NotificationKind AgentKind DetectSource Snapshot DaemonInfo AttachInfo ProtoError ErrorCode` and ids `WorkspaceId ProjectId SessionId SubAgentId`.
- Status enum: `run perm fail done idle unknown` (`Status::rank`, `needs_you`).
- Roll-up: `pub fn rollup(statuses: impl IntoIterator<Item = Status>) -> Status` (perm > fail > run > done > idle > unknown; empty → idle).
- Adapter: `trait Adapter: Send { fn name(&self) -> &'static str; fn agent(&self) -> AgentKind; fn source(&self) -> DetectSource; fn observe(&mut self, obs: &Observation) -> Vec<Detection>; }`; `Observation {Hook, Osc, Screen, Process, Tick}`; `Detection`; `Engine {new, with_default_adapters, register, observe, status, sub_agents, session_id, agent}` (bodies `todo!()`, tests `#[ignore]`); `StatusUpdate`; each stub module exposes `default_adapter(&SessionId, AgentKind) -> Option<Box<dyn Adapter>>`.
- `DaemonClient`: `connect close ping getSnapshot listProjects addProject updateProject removeProject listSessions createSession renameSession closeSession retrySession markRead resolvePermission attach detach sendInput resize getSettings updateSettings detectLaunchCommands getGitInfo hookEvent shutdown onConnection onEvent onPtyData writePty` (+ `ConnectionState`, `DaemonError`). Added contracts outside proto: `lib/ui/commands.ts` (`emitUiCommand/onUiCommand`: `openSettings openPalette openNewSession openAddProject toggleSidebar`), `lib/ui/selection.ts` (`selectSession`, `getSelectedSessionId`, `useSelectedSessionId`).

**Requests (shared/owned-elsewhere files):** none required. Optional: remove generated `app/src-tauri/icons/{android,ios,Square*.png,StoreLogo.png,icon.ico}` and `.playwright-mcp/` from the worktree (gitignored, I may not delete files).

**Open questions / notes for the orchestrator:**
1. Extra WP0 contract beyond the plan text: `lib/ui/commands.ts`, `lib/ui/selection.ts`, `DaemonClient.onConnection`, `MAX_HELLO_LEN`/`*_max` readers, `default_adapter` stubs, `rust-toolchain.toml` (1.95.0, edition 2024). All additive; say if you want any removed before freezing.
2. Plan vs reality: `@xterm/addon-canvas` not pinned (latest 0.7.0 needs `@xterm/xterm ^5`; we pin 6.0.0; WebGL + built-in DOM renderer remain). The plan says Rust 1.95; it is pinned via `rust-toolchain.toml`.
3. `app/src/lib` was silently ignored by a global gitignore on this machine (`lib/`); `.gitignore` now has `!app/src/lib/`. Check `git status` shows `app/src/lib/**` before committing.
4. `npm install` reports 11 moderate advisories (dev toolchain, not audited in detail) and a `fsevents` install-script warning from npm 11; no runtime impact seen. Not triaged.
5. Left open from review (NICE-TO-HAVE): TS has no runtime check that `types.ts` field names match the Rust fixtures beyond method coverage and MockClient seeding from `snapshot_mock.json`; `npm run tauri dev` is fixed to port 1420 (documented in CLAUDE.md).
6. Tokens/cost in the status bar shows `—` as decided; the right-panel icons are omitted (Beta).

**Unverified:** `npm run tauri dev` window (no GUI session; `tauri build --debug --no-bundle` compiles and the capability/CSP/config validate, but the window drag with the overlay title bar was not exercised at runtime); CI on GitHub (`macos-14` not run; the identical commands pass locally); real daemon (stub); `harnex:validator` pass was skipped (headless run; I ran the full command set myself).
