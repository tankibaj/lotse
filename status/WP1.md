# WP1 · Daemon core

| Field | Value |
|---|---|
| Agent / run | claude CLI (harnex, headless) · wp1-daemon · session n/a |
| Worktree / branch | ~/Repos/incubator/lotse-wt/wp1-daemon · feature/wp1-daemon |
| Started | 2026-11-02 03:10 |
| ETA (first) | 05:00 (≈ 110 min) |
| ETA (current) | done 03:55 |
| Phase | done (uncommitted in worktree, orchestrator commits) |
| Progress | 9 / 9 tasks · 100 % |
| Last update | 2026-11-02 03:55 |

## Tasks
- [x] config/paths, token, socket dir/modes, pidfile, detach, shutdown
- [x] SQLite schema + migrations (projects, sessions, settings)
- [x] PTY sessions (spawn/input/resize/kill, ring buffer, exit)
- [x] socket server: hello/token, requests, events, PTY frames, multi-client
- [x] launch commands: tokenizer, PATH lookup, version detection (3 s timeout), login env
- [x] title from first prompt (editable)
- [x] integration tests (done-when list)
- [x] CI commands green (fmt, clippy, test)
- [x] gates (validator, reviewer ×2 rounds), parity table, final report

## Log (newest last, `HH:MM phase — note`)
- 03:10 plan — read ALPHA_PLAN WP1, proto, detect, git stubs. Design: lib + bin, std threads for PTY, tokio for socket, per-session state mutex, per-client outbound queue.
- 03:25 implement — started
- 03:34 test — own run green. Bugs found by tests and fixed: derive_title dropped tabs, env capture kept a trailing newline, connection stayed open after a bad frame (writer task held its own sender), a test fixture deadlock.
- 03:37 review — harnex validator (PASS, 62/62 ×4) + reviewer (CHANGES: 5 MUST-FIX, 4 NICE) in parallel
- 03:48 review round 2 — all 5 MUST-FIX fixed (kill escalation + foreground group, replay > queue limit, stale `session_created` meta/event order, attach-order/slow-client/escalation tests, flaky pid waits) plus NICE items 6-8. Delta reviewer found one regression (stale pgid reused by another process after exit): fixed in `kill()` and `on_exit`.
- 03:54 test — final CI set green: fmt, clippy -D warnings, `cargo test --workspace`: lotsed 29 unit + 39 integration (lifecycle 6, security 9, sessions 18, settings 6); lotse-proto 22; 3 stub tests ignored. 6 consecutive `cargo test -p lotsed` runs 68/68.
- 03:55 done

## Handoffs and bounce-backs
| Time | Type | From → To | Reason | Resolved |
|---|---|---|---|---|
| 03:44 | bounce-back | reviewer → WP1 | CHANGES: kill escalation, replay>queue, stale meta, test gaps, flaky waits | 03:50 fixed |
| 03:50 | bounce-back | reviewer (delta) → WP1 | regression: stale pgid signalled after exit | 03:53 fixed (self-verified, no third review round) |
| 03:50 | bounce-back | validator → WP1 | 2 flaky sessions tests under parallel load (leftover test assumption, resize) | 03:53 fixed |

## Blockers / requests for shared files
- none blocking. See "Requests" in the final report.

## Final report

**Files** (all `crates/lotsed/**`, nothing outside; `Cargo.toml`/`Cargo.lock` untouched): src 14 (13 new: config, db, git, hub, launch, lib, lifecycle, ring, secure, server, session, state, title; `main.rs` rewritten), tests 5 (`common/mod.rs`, lifecycle, security, sessions, settings).

**Done**
- `lotsed` lib + bin. Unix socket in a 0700 dir, socket/token/pidfile/db/log 0600, 256-bit token, constant-time compare, peer-uid check, hello within 5 s and ≤64 KiB before auth, version check. No TCP anywhere.
- Multi-client: one ordered outbound queue per client (`attached` response → replay → live bytes), events broadcast, slow client (>64 MiB queued) dropped.
- Registry: projects (path canonicalized, unique, palette/icon defaults from the mock, `.git` detection), sessions (spawn/attach/detach/input/resize/close/retry/rename/mark-read), settings (validated, empty command = reset to default), snapshot with daemon info.
- PTY via portable-pty: login-shell env (`$SHELL -l -i -c env`, 10 s, cached, falls back to the daemon env), `TERM`/`COLORTERM`, `LOTSE_SESSION_ID/PROJECT_ID/SOCKET/TOKEN_FILE` (token value never exported), raw-byte ring (settings, default 4 MiB, 64 KiB–32 MiB), exit → `session_exited` + `status_changed` (Done/Fail, source Process), kill = SIGHUP to leader + foreground groups, SIGKILL after 2 s, input queue bounded (8 MiB).
- SQLite (rusqlite bundled): migrations by `user_version`, newer schema refused, default workspace `w1`. Sessions are pruned at daemon start (PTYs are gone).
- Launch commands: tokenizer (quotes, `$VAR`, `~`), `$SHELL -l` default, PATH lookup (login env PATH), `--version` probe with 3 s timeout in its own process group, parallel for the four agents.
- Title: placeholder `New session` → first submitted line the user types (escape/backspace aware) or `first_prompt`; `rename_session` switches auto off; persisted.
- `--detach` (own session via setsid, log file 0600, waits for the socket), pidfile with flock (single instance, stale files ignored), SIGTERM/SIGINT/`shutdown` = graceful (listener closed, sessions killed, socket + pidfile removed), SIGHUP ignored.

**Tests (numbers from my runs, validator got 62/62 ×4 before round 2)**
- fmt --check clean · clippy -D warnings clean · `cargo test --workspace`: lotsed 29 unit + 39 integration = 68 pass, lotse-proto 22 pass, 3 ignored (WP2/WP7 stubs). 6 consecutive runs of `cargo test -p lotsed`: 68/68.
- Done-when mapping: `sessions::full_flow_create_spawn_echo_detach_reattach_replays_kill` · `client_restart_keeps_the_session_and_its_output` · `security::socket_token_pidfile_database_and_directory_modes` · `security::the_daemon_has_no_tcp_listener` (`lsof -nP -iTCP|-iUDP|-i -a -p <pid>` empty, after a sanity check that lsof sees `lotsed.sock`).
- Red evidence: the SIGHUP-immune test failed (37 ms) against the first escalation, the process-group test failed when the PTY master was closed before the signals.

**Parity (non-UI WP: compare behaviour with `spec/mock.html`; no screenshots, no UI)**

| Feature | Mock | App (daemon) | Match |
|---|---|---|---|
| Status names / roll-up | run perm fail done idle unknown, perm>fail>run>done>idle | proto enum used; daemon sets Idle (shell) / Unknown (agent) at start, Done/Fail on exit | match; run/perm/sub-agents come from detection (WP2/6/8) |
| Launch defaults | `$SHELL -l`, `claude`, `codex`, `opencode` | same strings, Reset target = `default_command` | match (test) |
| Version text | `zsh 5.9`, `v2.1.309`, `v0.98` | `zsh 5.9` / `v2.1.309` format | match |
| "not found on PATH" | OpenCode row | `found=false`, no path/version | match |
| `· hooks installed`, `· OSC 7501` | shown on Claude/Codex rows | `integration: None` | gap → WP2/WP6/WP8 (accepted, detection WPs own it) |
| Runs in project folder with login env | Settings › Agents hint | cwd = project path, login env | match (test with fake shell) |
| Project data | 15 swatch keys, icon `folder`, workspace `w1`, non-git `notes` | palette cycle, icon `folder`, `w1`, `.git` check | match |
| Footer "lotsed running · N sessions" | daemon + session count | `snapshot` has daemon pid/version/started_at + sessions | match |
| Card title from first prompt, editable | yes | yes | match |
| Git line (branch, ↑, ±, worktree) | shown | `is_git` only; branch empty, 0, 0 | gap → lotse-git is a `todo!()` stub (WP7), wired in WP8 (accepted) |
| Unread dot, perm card, sub-agent fold | shown | not produced | gap → needs detection (WP2/8) (accepted) |
| Allow/Always/Deny | buttons | `resolve_permission` acks, no effect | by design (Alpha only focuses the terminal) |
| Retry on fail card | button | respawns an exited session; `Conflict` while the process still runs | partial, see open questions |
| New session with worktree | option | `Unavailable` | gap → WP7 (accepted) |

**Requests for shared files** (none done by me)
1. WP8: swap `lotsed::git::info` for `lotse_git::git_info` once WP7 lands; feed `lotse_detect::Engine` from `State::handle` (`HookEvent` arm, currently acked), `SessionHandle::on_output` (OSC/screen) and `on_exit` (process).
2. WP2 (`lotse-hook`): the daemon exports `LOTSE_SESSION_ID`, `LOTSE_SOCKET`, `LOTSE_TOKEN_FILE` (path), `LOTSE_PROJECT_ID`; read the token from that file, never from env.
3. Plan gap: Decisions say the daemon feeds an `alacritty_terminal` VT model for screen detection; no WP lists it. Not done in WP1 (belongs with WP6/WP8).
4. Nothing for proto, Cargo.toml or lockfiles.

**Open questions / defaults I took**
- Retry of a still-running agent session (fail card on a live process) returns `Conflict`; the plan only says "re-run the last command". Needs a decision with WP8.
- Sessions are deleted at daemon start rather than restored as exited rows.
- `scrollback_bytes` limits 64 KiB–32 MiB (replay must fit in the 64 MiB client queue).
- A shell session's first typed line becomes its title (placeholder `New session` until then).

**Unverified**
- Login env capture against a real zsh/fish (tested with a fake shell script only); a `.zshrc` slower than 10 s falls back to the daemon env until restart.
- `first_prompt` typing (waits for first output + 700 ms, then text, 150 ms, CR) with real Claude Code / Codex / OpenCode.
- Background jobs that job control puts in their own process group are reached only through the shell's own SIGHUP forwarding (a `nohup` background job of an interactive bash can survive a close).
- No separate third review round after the pgid-reuse fix (cap: self-verified by tests and reading).
- Pidfile is unlinked while still flock-held at shutdown (reviewer's NICE item; harmless, left).
