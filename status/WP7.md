# WP7 · Git, notifications, native helpers

| Field | Value |
|---|---|
| Agent / run | claude CLI (harnex quick, headless) |
| Worktree / branch | ~/Repos/incubator/lotse-wt/wp7-git-notify · feature/wp7-git-notify |
| Started | 2026-11-02 03:47 |
| ETA (first) | 04:45 (≈ 60 min) |
| ETA (current) | done |
| Phase | done |
| Progress | 6 / 6 tasks · 100 % |
| Last update | 2026-11-02 04:20 |

## Tasks
- [x] lotse-git: git_info (branch, ahead, dirty, worktree, no git) + tests on temp repos
- [x] lotse-git: create_worktree under ~/.lotse/worktrees/<project>/<branch> + tests
- [x] notify.rs: decision logic (toggles, per-project mode, pause, focus) + unit tests
- [x] notify.rs: Tauri adapter (notification, Dock badge, commands) + Requests for WP8
- [x] native/symbols: Swift SF Symbols helper (NOT compiled, see Unverified)
- [x] parity table, manual notification check, final report

## Log (newest last, `HH:MM phase — note`)
- 03:47 plan — started, ETA 04:45
- 03:58 test — lotse-git 17 tests pass; notify.rs needs `pub mod notify;` in lib.rs (WP8's) so I wired lib.rs TEMPORARILY, tested, reverted (lib.rs has no diff).
- 04:05 test — lotse-app notify: 14 tests pass, clippy clean. Manual run of the real app: notification + badge calls returned Ok.
- 04:10 review — harnex:reviewer (sonnet): APPROVE, 5 NICE-TO-HAVE.
- 04:20 done — fixed 4 of 5 NICE-TO-HAVE (tag/branch name, leftover dir, afplay zombie, Swift size key); lotse-git 20 tests; notify 14 tests (re-run after wiring again, reverted).

## Handoffs and bounce-backs
| Time | Type | From → To | Reason | Resolved |
|---|---|---|---|---|
| 04:10 | review | harnex:reviewer → WP7 | APPROVE, 5 NICE-TO-HAVE | 4 fixed, 1 documented as a WP8 request (set_settings on connect/reconnect) |

## Blockers / requests for shared files
See the Requests in the final report.

## Final report

**Files (all inside owns-paths):** lotse-git 1 modified (`crates/lotse-git/src/lib.rs`); app/src-tauri 1 new (`src/notify.rs`); native 2 new (`native/symbols/main.swift`, `build.sh`). No lib.rs/Cargo/lockfile change.

**Done**
- `git_info(dir)`: branch (`branch --show-current`), ahead (`@{upstream}..HEAD`, 0 without upstream), dirty (`status --porcelain` lines), worktree (git-dir ≠ common-dir) + `worktree_path`, "No git" (plain folder, bare repo). Env: `GIT_OPTIONAL_LOCKS=0`, `LC_ALL=C`, no prompts.
- `create_worktree(repo, branch)` → `~/.lotse/worktrees/<project>/<branch>` (`create_worktree_in` takes a root for tests). Branch validated with `check-ref-format` (no `..`, no leading `-`/`/`); new branch from HEAD or reuse existing; refuses existing path; leading `lotse/` dropped from the folder name (mock tooltip `~/.lotse/worktrees/<project>/<branch without lotse/>`).
- `app/src-tauri/src/notify.rs`: pure `resolve` (global / Custom / Mute, mock `notifies()`), `gate` (pause bell, only-when-unfocused), `decide`, `decide_event`; adapter `NotifyState`, `on_event`, commands `set_badge` (Dock badge) and `set_focused_session`.
- `native/symbols`: Swift helper `lotse-symbols <name> <size> <weight> <out-dir>` → template PNG @1x/@2x cache.

**Tests**
- `cargo test -p lotse-git`: 20 passed (no git, missing dir, clean, subdir, unborn, dirty=3, ahead=2 vs local bare upstream, detached, bare, tag-named-like-branch, worktree add/reuse/lotse-prefix/dirty per worktree/from linked worktree/existing path/checked-out branch/unsafe names/not a repo/failed add leaves nothing).
- `cargo test -p lotse-app` (with temporary `pub mod notify;`): 14 passed. Not run in CI until WP8 adds the module.
- `cargo test --workspace` (lib.rs as on main): all pass (lotse-git 18 at that run, +2 after reviewer fixes; lotse-git re-run 20/20). `cargo fmt --all --check` clean; `cargo clippy --workspace --all-targets -- -D warnings` clean; clippy for lotse-app with notify wired also clean.
- harnex:reviewer (sonnet): APPROVE, 5 NICE-TO-HAVE, 4 fixed.

**Manual notification check:** with lib.rs temporarily wired (plugin + state + a setup thread), `cargo run -p lotse-app` fired `notify::on_event(Event::Notification{perm, "praxis-web · Claude Code", "Needs permission", "Bash(npx playwright test e2e/patients)", sound})`, a direct plugin `show()`, and `set_badge(3)`: `show()` → `Ok(())`, `set_badge(3)` → `Ok(())`, process exited 0. **I could not see the screen (headless run): banner appearance, sound and the Dock badge are NOT visually confirmed.** lib.rs was reverted afterwards. Naim: please eyeball once after WP8 wires it. Per the plugin source (`tauri-plugin-notification-2.8.0/src/desktop.rs:520`) dev builds use bundle id `com.apple.Terminal`, so banners show as Terminal (the accepted risk).

**Parity (non-UI: behaviour vs `spec/mock.html`)**
| Feature | Mock | App (WP7) | Result |
|---|---|---|---|
| Git line fields | branch, `↑n` if >0, `±n` if >0 | `GitInfo{branch, ahead, dirty}`, 0 = hidden | match |
| "No git" | folder icon + "No git" | `is_git:false` for plain folder/bare repo | match |
| Worktree vs main checkout | icon + tooltip "Isolated worktree · ~/.lotse/worktrees/<p>/<b w/o lotse/>" | `worktree` + `worktree_path`; path created with that layout | match |
| Worktree create toggle | "Create isolated git worktree" + branch field | `create_worktree(repo, branch)` | match (UI is WP5/WP8) |
| Notification events | perm, fail, done, sub-agent done | `NotificationKind` ×4 | match |
| Global defaults | perm/fail banner+sound, done banner only, sub-agent off | `resolve` tests assert exactly this | match |
| Per-project mode | global / custom per event / mute | `NotificationMode` handled in `resolve` | match |
| Pause bell ⌥⌘N | pauses all banners | `settings.paused` → `Skip::Paused`, beats everything | match |
| "Only when Lotse is not focused on that session" | checkbox, default on | `gate`: skip only if window focused AND same session | match |
| Dock badge | need-you count (perm+fail) | `set_badge(count)`, 0 clears | match (count source: `subscribeNeedYouCount`) |
| Banner text | title "praxis-web · Claude Code", "Needs permission", mono body | title; body = subtitle + "\n" + body (plugin has no subtitle) | accepted deviation: plugin API has title+body only; mock's 3-line banner with Allow/Deny is Beta |
| Screenshots `parity/wp7/` | n/a | none: non-UI WP, no screen | n/a |

**Requests (shared files; for WP8 / orchestrator)**
1. `app/src-tauri/src/lib.rs`: add `pub mod notify;` and in the builder chain
   `.plugin(tauri_plugin_notification::init())`, `.manage(notify::NotifyState::default())`, `.invoke_handler(tauri::generate_handler![notify::set_badge, notify::set_focused_session /*, WP8's own */])` (verified to compile and pass clippy).
2. In the daemon bridge: call `notify::on_event(&app, &event)` for every daemon `Event`; on connect and on `reconnected` also call `app.state::<notify::NotifyState>().set_settings(snapshot.settings.notifications.clone())` (otherwise a persisted pause is ignored until the next `SettingsChanged`).
3. Webview (`app/src`): `subscribeNeedYouCount(client, n => invoke('set_badge', { count: n }))`; on selected-session / window-focus change `invoke('set_focused_session', { sessionId })`.
4. ⌥⌘N: send `UpdateSettings` with `notifications.paused` toggled (the daemon echoes `SettingsChanged`, which `on_event` consumes).
5. Optional: add `native/symbols/build.sh` to the build flow later (`sh native/symbols/build.sh`; not executable-bit set, I could not chmod).

**Open questions / notes**
- `Event::Notification` has no `banner` field, so `on_event` always shows a banner; the sound-only path (`resolve` → banner off, sound on, `afplay`) is reachable only via `decide`. If WP1 should emit sound-only events, proto needs a field.
- I assumed the daemon applies per-project overrides + global toggles before emitting `Event::Notification` (per the proto comment); `resolve`/`decide` exist if it does not.
- Plan vs mock: plan says `<branch>`, mock tooltip strips `lotse/`; I followed the mock for the folder name.
- Two repos with the same folder name share `<root>/<project>/<branch>`: second gets `PathExists` (fails safe).

**Unverified**
- Swift helper never compiled or run (swiftc needs approval in this run). Check: `sh native/symbols/build.sh`, then `native/symbols/build/lotse-symbols folder 16 regular /tmp/x`; API docs: https://developer.apple.com/documentation/appkit/nsimage/init(systemsymbolname:accessibilitydescription:) and `.../nsimage/symbolconfiguration`.
- `.sound("Default")` reaching the macOS default sound via notify-rust/mac-notification-sys; visual banner/badge (see above); `afplay` sound-only path.
- Notification identity in unsigned dev builds (Terminal) — known, accepted.
