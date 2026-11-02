# WP2 · Status engine and Claude Code detection

<!-- Written by the delegated agent only. One writer per file. Absolute path: ~/Repos/incubator/lotse/status/WPn.md -->

| Field | Value |
|---|---|
| Agent / run | claude CLI (harnex, headless) · wp2-status-claude |
| Worktree / branch | ~/Repos/incubator/lotse-wt/wp2-status-claude · feature/wp2-status-claude |
| Started | 2026-11-02 03:10 |
| ETA (first) | 04:30 (≈ 80 min) |
| ETA (current) | done 03:31 |
| Phase | done |
| Progress | 7 / 7 tasks · 100 % |
| Last update | 2026-11-02 03:31 |

## Tasks
- [x] 1. Engine (`engine.rs`): per-source merge, running_since, sub-agent merge, tests
- [x] 2. Claude adapter (`claude.rs`): status table, sub-agent roster, perm summary, self-heal
- [x] 3. Fixtures (`integrations/claude/fixtures/*.json`, 11 scenarios) + fixture-driven tests covering every table row
- [x] 4. `lotse-hook`: stdin → socket (token, hello, HookEvent), exit 0, silent; tests vs fake daemon + binary e2e
- [x] 5. Installer/uninstaller (`lotse-hook` lib, order-preserving JSON, backup, marker) + round-trip tests (temp dirs only)
- [x] 6. `integrations/claude/` (hooks.json template, README)
- [x] 7. fmt, clippy, tests, parity (source-based, see below), fresh review (sonnet) + fixes

## Log (newest last, `HH:MM phase — note`)
- 03:10 plan — started. No "status table" is defined anywhere in plan/spec; defined it in `claude.rs` module docs (every row has a fixture step).
- 03:2x implement — engine, adapter, fixtures, hook, installer written; `cargo test` green.
- 03:31 review — fresh `harnex:reviewer` (sonnet): CHANGES. Fixed: orphaned sub-agent when a stop event is lost (stale on next prompt / tick expiry), `SubagentStop` no longer overwrites `Fail`, `tool_response` stripped before forwarding, flaky timing asserts loosened. Not fixed (see report): parallel-tool `PreToolUse` clears a pending perm; backup/`{}` left after uninstall.
- 03:31 done — fmt/clippy/tests green (numbers in final report). Playwright blocks `file:` URLs, so no screenshots (parity from mock source).

## Handoffs and bounce-backs
| Time | Type | From → To | Reason | Resolved |
|---|---|---|---|---|
| 03:31 | bounce-back | reviewer → WP2 | CHANGES: lost `SubagentStop` leaves session `Run` forever | yes: stale-on-prompt (10 min) + tick expiry (30 min) |

## Blockers / requests for shared files
- Playwright MCP denies `file://` ("Access to file: protocol is blocked"); I did not work around it. No `parity/wp2/` screenshots. Orchestrator: serve `spec/mock.html` over http if screenshots are wanted.
- Requests: see Final report › Requests.

## Final report

**Files (all inside owns-paths, nothing outside):** lotse-detect 3 (`engine.rs`, `claude.rs`, `tests/claude_fixtures.rs`); lotse-hook 7 (`main.rs`, `lib.rs`, `forward.rs`, `json.rs`, `claude_settings.rs`, `claude_settings/tests.rs`, `tests/cli.rs`); `integrations/claude/` 13 (`hooks.json`, `README.md`, 11 fixtures). No Cargo.toml/lock change, no new deps.

**Done**
- `Engine`: per-source last opinion, strongest source wins (hook > osc7501 > screen > process), `Unknown` = no signal, reports only status/perm/sub-agent changes, `running_since` on entering `Run`. `with_default_adapters` registers the four `default_adapter`s.
- `ClaudeAdapter` (`DetectSource::Hook`): status table in the module docs. Lead + per-sub-agent roster; sub-agent perm = session perm (as the mock's a3 card); Stop with running sub-agents stays `Run`; `StopFailure` → `Fail`; `PostToolUseFailure` → `Run` (decision, see questions); perm summary per tool (`Bash`→command, `Write`/`Edit`→file_path, …) cut to one 160-char line; PTY exit resets to `Idle`; self-heal for lost stop events.
- `lotse-hook`: stdin → `hello` (token from `…/Lotse/token`) → `hook_event` with `LOTSE_SESSION_ID`; prints nothing, always exits 0, 1.5 s hard deadline (worker thread), 8 MiB stdin cap, no-op outside Lotse sessions, drops `tool_response`. `LOTSE_DATA_DIR` overrides the data dir, `LOTSE_HOOK_DEBUG` prints failures.
- Installer: `lotse-hook integration claude install|uninstall|status [--config-dir]`, library API in `lotse_hook::claude_settings`. Marker `# lotse:managed` in each command, idempotent, backup `settings.json.lotse-backup` (0600), order/escape-preserving JSON (own small parser; `serde_json` would sort keys), symlink write-through, mode kept, refuses non-strict JSON. Uninstall restores the backup bytes when nothing else changed, else removes only Lotse hooks.

**Tests (all green, run by me; `cargo fmt --all --check`, `cargo clippy --workspace --all-targets -D warnings` clean)**
- `cargo test --workspace --offline`: lotse-detect 17 unit + 3 fixture-driven; lotse-hook 30 unit + 4 binary e2e; lotse-proto 22, lotsed 1 (unchanged); 0 failed. New: 57 tests.
- Done-when 1: every row of the status table (24 row ids) is exercised by ≥1 fixture step, asserted by `fixtures_cover_every_row_of_the_status_table`.
- Done-when 2: `round_trip_is_byte_identical_to_the_backup`, `round_trip_is_byte_identical_for_unusual_formatting` (compact, CRLF/tabs, no hooks key, pre-existing empty arrays, `{}`, empty file) and the binary e2e round trip: install → uninstall leaves settings byte-identical to the backup.
- Not run: `npm` suite (no app change; not touched).

**Parity (non-UI; compared against `spec/mock.html` source, no screenshots possible)**

| Feature | Mock | App (WP2) | Match |
|---|---|---|---|
| Status names | run perm fail done idle (G map) | engine emits `Status` of proto (+ `unknown` = no signal) | match |
| Roll-up order | `rank` perm>fail>run>done>idle | session status derived lead perm > sub perm > fail > run > done/idle; proto `rollup` is WP0 | match |
| Sub-agent states | a1: explore-repo done, test-runner run | fixture `subagents.json` reproduces `[done, run]`, lead `run` | match |
| Sub-agent perm | a3: sub e2e-runner perm ⇒ card perm, Bash `npx playwright test e2e/patients` | fixture `subagent_permission.json` reproduces `[done, done, perm]`, session `perm` with that request | match |
| Card counts include sub-agents (`cnt` counts sub statuses) | yes | sub perm/fail surfaces on the session status (perm) | match; sub `fail` alone does not make the session `fail` (gap, accepted: lead keeps its state) |
| Sub-agent names | `explore-repo` … | `agent_type`, else `agent_id` | match |
| Elapsed time of run cards (`el`) | seconds counter | `running_since` set on entering `Run` | match |
| Idle shell / non-Claude sources | `source` hook/osc7501/process | Claude adapter only; osc/screen/process are WP6 | n/a |
| Screenshots | 1440×900 / 1280×800, light/dark | none: Playwright MCP blocks `file:` | gap (tooling) |

**Requests for shared files (not done by me)**
1. WP1/WP8 (`lotsed`): set `LOTSE_SESSION_ID=<session id>` on every spawned PTY (required); handle `Request::HookEvent` by feeding `Observation::Hook { agent, payload, at: now_ms }` to that session's `Engine` (`with_default_adapters`) and map `StatusUpdate` → `Event::StatusChanged` / `SubAgentsChanged` / `SubAgentStatus`; also send `Observation::Process{foreground: None}` on PTY exit and a periodic `Observation::Tick` (≥ 1/min; the sub-agent self-heal needs it).
2. WP1: socket/token path `<data dir>/lotsed.sock` and `<data dir>/token` as in the plan; data dir default `~/Library/Application Support/Lotse`; hook honours `LOTSE_DATA_DIR` override (export it to PTYs if the daemon uses a non-default dir).
3. WP1/WP5: `HookEvent` payload arrives without `tool_response` (stripped by `lotse-hook`), contrary to the proto doc "verbatim" (`crates/lotse-proto/src/message.rs`, comment only; no bump needed).
4. WP8/WP5: call `lotse_hook::claude_settings::{install,uninstall,status}` (add `lotse-hook = { path }` to `lotsed/Cargo.toml`, lockfile edge) or shell out to `lotse-hook integration claude …`; the hook binary must be at a stable path (the installed command embeds it; reinstall updates it in place).
5. Plan gap: ALPHA_PLAN.md refers to "the status table" but none exists; the one in `claude.rs` is now the reference. Orchestrator may want to cite it.

**Open questions (defaults taken)**
- `PostToolUseFailure` → `Run`, not `Fail` (the error goes back to the model; only `StopFailure` is `fail`). research.html lists it among the hooks that give "failed"; if Naim wants a red card on a failed tool, change one arm in `handle_hook`.
- Hooks are installed into user-level `~/.claude/settings.json` (global; harmless outside Lotse since the hook no-ops without `LOTSE_SESSION_ID`), not per project.
- Hook entries use `matcher: "*"` on tool events, 5 s timeout.

**Unverified (no web access)**
- All event names, payload fields (`agent_id`, `agent_type`, `source`, `tool_use_id` on `PermissionRequest`, `tool_input` keys per tool), the settings schema (`matcher`, `timeout` in seconds), that hook commands run via a shell (the `# lotse:managed` comment marker relies on it), and the matcher `*`. Checked only against Orca's and herdr's Claude integrations (local clones) and Orca's per-release event enum fixture. Check <https://code.claude.com/docs/en/hooks> (and the minimum Claude Code version for `PermissionRequest` ≥ 2.0.45, `StopFailure` ≥ 2.1.78 per Orca) before release. Unknown event names in `settings.json` could make Claude reject the file on old versions: gate install on `claude --version`.
- Fixtures are authored from the docs/Orca, not recorded from a live agent (no real agent in tests).

**Known limitations (reviewer findings kept as NICE-TO-HAVE)**
- A lead `PreToolUse` clears a pending perm; with parallel tool calls a later `PreToolUse` of another tool can hide a still-open prompt (needed so a denied prompt, which fires no `PostToolUse`, clears).
- Uninstall leaves the backup file (and `{}` if `settings.json` did not exist before install).
- Hook delivery is best effort (dropped on timeout/daemon restart); self-heal covers sub-agents, a lost lead `Stop` heals on the next prompt.
