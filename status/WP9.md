# WP9 · Hardening and Alpha build

| Field | Value |
|---|---|
| Agent / run | claude CLI · harnex quick (headless) |
| Worktree / branch | ~/Repos/incubator/lotse-wt/wp9-hardening · feature/wp9-hardening |
| Started | 2026-11-02 05:03 |
| ETA (first) | 07:30 (≈ 150 min) |
| ETA (current) | done |
| Phase | done |
| Progress | 9 / 9 tasks · 100 % |
| Last update | 2026-11-02 05:21 (the sandbox clock barely moved; wall time was longer) |

## Tasks
- [x] 1 Security baseline
- [x] 2 Audits
- [x] 3 THIRD_PARTY_NOTICES.md + LICENSE-herdr
- [x] 4 Packaging: signed `.app` built; bundled-binary launch NOT run (denied, see Unverified)
- [x] 5 README
- [x] 6 DOGFOOD.md
- [x] 7 Issue templates
- [x] 8 Flake hunt: 30 vitest runs, 0 failures
- [x] 9 Full suite + Playwright + parity + review (CHANGES → fixed)

## Log (newest last, `HH:MM phase — note`)
- 05:05 plan — started
- 05:08 implement — `bundle.macOS.files` puts lotsed + lotse-hook in Contents/MacOS. `sh`, shell loops and launching built binaries are denied in this run.
- 05:11 implement — capabilities: `core:default` → 5 explicit permissions.
- 05:13 implement — hook forwarded full Claude payload (prompt, last_assistant_message, tool_input contents) to the daemon; added `minimize_claude` + test (red → green).
- 05:15 test — vitest ×10 serial green; cargo suite: `sessions::a_client_that_stops_reading…` failed once while 3 vitest chains + cargo ran together (load), passed on rerun.
- 05:17 test — vitest ×20 more (2 parallel chains, under cargo load) green; cargo `--no-fail-fast` green.
- 05:19 parity — screenshots in `parity/wp9/`; Playwright 52/52 + 12/12.
- 05:20 review — harnex:reviewer (sonnet): CHANGES, 1 MUST-FIX (README uninstall killed the daemon before quitting the app) + 4 NICE. Fixed MUST-FIX, worktree step, privacy wording, licences.mjs exit code, capability test.
- 05:21 done.

## Handoffs and bounce-backs
| Time | Type | From → To | Reason | Resolved |
|---|---|---|---|---|
| 05:20 | bounce-back | reviewer → me | README uninstall order (kill daemon, app restarts it); worktree `rm -rf` in the copy-paste block; privacy wording overstated; `licenses.mjs` always exit 1 | yes |

## Blockers / requests for shared files
See Final report › Requests.

## Final report

**Files changed** (uncommitted, in the worktree): config 2 (`app/src-tauri/tauri.conf.json`, `capabilities/default.json`), Rust 2 (`crates/lotse-hook/src/forward.rs`, `tests/cli.rs`), new tests 2 (`app/src-tauri/tests/security_config.rs`, `app/e2e/wp9-parity.spec.ts`), scripts 2 (`scripts/build-app.sh`, `scripts/licenses.mjs`), docs 5 (`README.md`, `DOGFOOD.md`, `THIRD_PARTY_NOTICES.md`, `CLAUDE.md` commands section, `integrations/manifests/LICENSE-herdr`), issue templates 2 (`.github/ISSUE_TEMPLATE/bug.yml`, `feedback.yml`), parity PNGs 12 in `parity/wp9/`. No lockfile, proto, CI workflow or `ALPHA_PLAN.md`/`spec`/`STATUS.md` change. Playwright rewrote 27 PNGs in `parity/wp3` (13) and `wp4` (14), and 4 in `wp8`: **restore with `git checkout -- parity/wp3 parity/wp4 parity/wp8` before committing**.

**Small security fixes (each with a test)**
1. `lotse-hook` data minimisation: the Claude hook forwarded the whole payload (user prompt, `last_assistant_message`, `tool_input` with file contents) to the daemon. Now it drops `prompt`, `last_assistant_message`, `message` and reduces `tool_input` to the keys the adapter summarises. Test: `forward::tests::claude_payloads_leave_conversation_text_and_file_contents_behind` (failed before the change).
2. Capabilities: `core:default` (path, image, menu, tray, resources, event emit, …) → `core:event:allow-listen`, `allow-unlisten`, `core:window:allow-start-dragging`, `allow-internal-toggle-maximize`, `dialog:allow-open`. Test: `security_config::capabilities_are_explicit_and_minimal`.
3. CSP: added `object-src 'none'; base-uri 'none'; form-action 'none'` (prod and dev). Test: `security_config::csp_has_no_eval_and_no_remote_origins`.
4. Test added for the OpenCode CLI install/uninstall incl. a foreign file with the same name (`cli::opencode_plugin_install_and_uninstall_leave_foreign_files_alone`).

**Checklist**

| # | Item | Result | Evidence |
|---|---|---|---|
| 1a | No TCP/UDP listener, daemon | ✓ | `lotsed/tests/security.rs::the_daemon_has_no_tcp_listener` (lsof `-iTCP`, `-iUDP`, `-i` on the daemon with sessions + clients, sanity-checks that lsof sees the socket) |
| 1b | No listener, app | ✓ (static) / ✗ live | no `TcpListener`/`UdpSocket`/`127.0.0.1` in `crates/**`, `app/src-tauri/src`, `app/src` (only the Vite dev config); `cargo tree --target aarch64-apple-darwin -i hyper/-i reqwest/-i tiny_http`: not in the macOS graph; live lsof on the bundled app **not run** (launch denied) |
| 1c | Modes | ✓ | `security.rs::socket_token_pidfile_database_and_directory_modes`: dir 0700; socket, token, pid, db 0600; uid = me; token 64 hex (256 bit); `a_loose_data_dir_is_tightened_on_start` |
| 1d | Every request needs the token | ✓ | `wrong_token_is_unauthorized_and_closed`, `requests_before_hello_get_nothing`, hello timeout (5 s), oversized pre-auth frame dropped, peer-uid check (`server.rs:74`), constant-time compare |
| 1e | Hooks install and uninstall cleanly | ✓ | Claude: `cli::integration_install_status_uninstall_round_trip` (temp HOME/config dir, byte-identical after uninstall) + the `claude_settings` unit tests; OpenCode: 4 `opencode_plugin` unit tests + the new CLI test |
| 1f | CSP | ✓ | prod: `default-src 'self'; script-src 'self'; style-src 'self' 'unsafe-inline'; img-src 'self' data:; font-src 'self'; connect-src ipc: http://ipc.localhost; object-src/base-uri/form-action 'none'`. No `unsafe-eval`, no remote origin (guarded by test). Accepted: `style-src 'unsafe-inline'` (React/xterm inline styles). devCsp adds `unsafe-inline` scripts and `ws://localhost:1420` (dev only) |
| 1g | Capabilities | ✓ | each permission and why: `core:event:allow-listen` (daemon events, drag-drop), `core:event:allow-unlisten` (cleanup), `core:window:allow-start-dragging` + `allow-internal-toggle-maximize` (`data-tauri-drag-region` title bar: drag, double-click zoom), `dialog:allow-open` (folder picker). No wildcards, no `:default` sets, no fs/shell/http/opener, no devtools. App commands need no permission. `tauri-plugin-fs` compiles in as a dependency of the dialog plugin but is granted nothing |
| 1h | Host commands | ✓ | `open_url`: http/https only, ≤4096, no space/control, one argv to `/usr/bin/open`. `clone_repo`: `git clone -- <url> <dest>`, URL not starting `-`, `GIT_ALLOW_PROTOCOL=file:git:http:https:ssh` (no `ext::`, tested), no prompts, dest absolute and non-existing. `reveal_in_finder`: absolute path (cannot be an option), exists. `scan_repos`/`create_project_dir`: absolute paths, `~` expanded, no shell, 500-child cap. `integration_*`: agent and action whitelisted, fixed argv. No injection found. Note: `create_project_dir`/`clone_repo` write anywhere the user can; the caller is the CSP-locked webview (accepted) |
| 2a | `cargo audit` | ✓ | 1395 advisories, 510 crates: **0 vulnerabilities**, 5 warnings (`proc-macro-error` RUSTSEC-2024-0370, `zbus_names` -2026-0371, `zvariant` -2026-0370, `zvariant_derive` -2026-0372 unmaintained; `glib 0.18.5` -2024-0429 unsound). None is in the macOS target graph (`cargo tree --target aarch64-apple-darwin -i glib -i zvariant …` → nothing): Linux-only via wry/GTK/notify-rust |
| 2b | `npm audit --omit=dev` | ✓ | 0 vulnerabilities |
| 2c | `npm audit` (full) | ✓ justified | 11 moderate, one root cause: `minimatch <11.0.1` (GHSA-g3hv-75wg-m249, DoS via globparts) under eslint/typescript-eslint. Dev-only lint tooling, never bundled or run on untrusted globs. The offered "fix" is `--force` to eslint@4 (a downgrade): not applied. No lockfile change |
| 3 | Licences | ✓ | `THIRD_PARTY_NOTICES.md` + `integrations/manifests/LICENSE-herdr` (Apache-2.0 text; herdr has no NOTICE and no holder in its LICENSE). Orca MIT text. Rust 275 runtime crates, npm 13: no GPL/LGPL/AGPL. **Flag: 5 MPL-2.0 crates** (`selectors`, `cssparser`, `cssparser-macros`, `dtoa-short`, `option-ext`), unmodified, file-level copyleft, accepted. `node scripts/licenses.mjs` regenerates the summary |
| 4 | Packaging | ✓ built and signed / ✗ launch | `bundle.macOS.files` → `Lotse.app/Contents/MacOS/{lotse-app 7.2 MiB, lotsed 5.5 MiB, lotse-hook 1.2 MiB}`. `codesign --force --deep -s -`; `--verify --deep --strict`: valid on disk, satisfies its Designated Requirement; `-dv`: `Identifier=dev.tankibaj.lotse`, `Signature=adhoc`, Info.plist bound, sealed resources 3 files; nested `lotsed` adhoc-signed. **Path: `/Users/naim/Repos/incubator/lotse-wt/wp9-hardening/target/release/bundle/macos/Lotse.app`** (a worktree path; the orchestrator should rebuild on `main`: `sh scripts/build-app.sh`). Size: not measured (`du` not allowed), binaries sum 13.9 MiB |
| 5 | README | ✓ | install (source build + `.app`), first run, hooks install/uninstall (UI and CLI), full uninstall in the right order (quit app → hooks → daemon → app → data → webview data; worktrees separate and manual), privacy |
| 6 | DOGFOOD.md | ✓ | exit criteria with tallies, manual checks per WP, security spot checks, the 4 decisions, known limits |
| 7 | Issue templates | ✓ | `bug.yml`, `feedback.yml`: version, macOS, agent + version, steps; no secrets asked, warning against pasting them |
| 8 | Flake | ✗ not caught | `npx vitest run` ×30 (10 serial; 20 as 2 parallel chains while cargo ran): 30/30 green, 25 files, 322 tests each. Not reproduced, no change made. **A different load-sensitive test failed once:** `lotsed/tests/sessions.rs::a_client_that_stops_reading_is_disconnected_instead_of_buffered_forever` (floods 200 MB, sleeps a fixed 5 s, expects the 64 MiB queue limit to have kicked the client; under 4 busy processes the flood is slower than 5 s). Green on 2 reruns at normal load. Suggest: poll instead of a fixed sleep |
| 9 | Open decisions | ✓ | listed in `DOGFOOD.md` § 3, unchanged |

**Test results** (final run, after the last edit): `cargo fmt --all --check` clean; `cargo clippy --workspace --all-targets -- -D warnings` clean; `cargo test --workspace --no-fail-fast` 267 passed, 0 failed, 1 ignored (18 test binaries; includes the 4 `security_config`, 1 new `cli`, 1 new `forward` tests); `npm run lint` clean; `npm run typecheck` clean; `npx vitest run` 25 files / 322 tests; `npm run build` ok; Playwright `-c e2e/playwright.all.config.ts` (port 1421) 52/52; `-c src/sheets/e2e/playwright.chrome.config.ts` 12/12. harnex reviewer: 1 MUST-FIX fixed, NICE-TO-HAVE: hook key list duplicates `summarize_input` (comment links them, no cross-test).

**Parity (non-UI WP: behaviour vs `spec/mock.html`; shots `parity/wp9/{mock,app}-{light,dark}-{main,agents,notifications}.png`, 1440×900)**

| Feature | Mock | App | Match |
|---|---|---|---|
| Settings › Agents | Terminal `$SHELL -l`, Claude Code "v2.1.309 · hooks installed", Codex "v0.98 · OSC 7501", OpenCode "not found on PATH", Reset links | same rows, text and colours (MockClient seed); real client adds Install/Remove hooks button | match (+addition, WP8) |
| Settings › Notifications | Needs permission B+S+Allow/Deny, Failed B+S, Done B, Sub-agent done none; "Only when Lotse is not focused" checked | identical defaults | match |
| Status names + roll-up | perm > fail > run > done > idle | same cards: perm (Allow/Always/Deny), fail, run, done, idle; sub-agent rows | match |
| Git line | branch, ↑ahead, ±dirty, worktree tag; "No git" | same | match |
| Window frame | 32 px rounded frame | full window | accepted (WP0 shell) |
| Status bar tokens/cost | "Today 412k tokens · $4.18" | "Today — tokens · —" | accepted (plan: Alpha shows `—`) |
| Collapsed roll-up of infra-terraform / dotfiles in default shot | collapsed | expanded by default | accepted state difference (WP3 compared the collapsed shot) |
| Right toolbar (Changes, Session, …) | present | absent | accepted (Beta) |
| Session count "6 sessions" | 6 | 7 (app counts the seed's 7) | accepted (mock text is hand-written) |

**Requests (shared files / orchestrator)**
1. `chmod +x scripts/build-app.sh` (not possible here); until then `sh scripts/build-app.sh`.
2. Restore PNGs: `git checkout -- parity/wp3 parity/wp4 parity/wp8`.
3. Decide on the `sessions.rs` flake fix (replace fixed 5 s sleep with polling; test file is WP1's, not mine).
4. Optional hardening not done (out of my paths): gate Claude hook install on `claude --version` (WP2 noted unknown hook names may make old Claude Code reject `settings.json`); the Claude hook key list could be exported from `lotse-detect` and shared.
5. `ALPHA_PLAN.md` mentions `tauri-plugin-opener`; the app uses `/usr/bin/open` (decision 4 in DOGFOOD.md).

**Open questions**: the four decisions in `DOGFOOD.md` § 3. Defaults taken: kept all four as they are.

**Unverified**
- **Bundled app launch and live no-listener check**: starting `Lotse.app/Contents/MacOS/lotse-app --isolated-data-dir …` (and `lotsed` directly) was denied by the run's permissions, so "daemon socket appears from the bundle" and `lsof` on the running app were not observed. Covered instead by: `app/src-tauri/tests/bridge.rs` (app starts its sibling daemon, flow, relaunch, with the debug binaries), the bundle layout check (siblings in `Contents/MacOS`, which is where `sibling_binary` looks), codesign verification. `scripts/build-app.sh` does this check (smoke step) but I could not run the script (`sh` denied). Naim: `sh scripts/build-app.sh` on `main`.
- Tightened capabilities at runtime (event listen, drag-drop, title-bar drag and double-click zoom): identifiers validated by the build, usage reviewed against the code, but the window was not driven. DOGFOOD § 2 has the check. If `internal-toggle-maximize` is not the identifier needed for double-click zoom, only that gesture is affected.
- New CSP directives in a real WKWebView: guarded by test, not rendered.
- `.app` size, `Info.plist` minimum macOS version (left at Tauri's default; README says "macOS, Apple Silicon").
- Gatekeeper behaviour on a quarantined copy (README advice is standard, not tested).
- Red check of `security_config` tests on base: fail by construction (no `object-src`, `core:default`, no `files`), not executed.
- `harnex.mjs untracked save/new` not run (no baseline at start); new untracked files are the ones listed above; `app/dist`, `target/`, `.playwright-mcp` are ignored.
