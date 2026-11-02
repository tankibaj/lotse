# WP6 · Codex and OpenCode detection

<!-- Written by the delegated agent only. One writer per file. Absolute path: ~/Repos/incubator/lotse/status/WP6.md -->

| Field | Value |
|---|---|
| Agent / run | claude CLI (harnex quick, headless) · session wp6 |
| Worktree / branch | ~/Repos/incubator/lotse-wt/wp6-codex-opencode · feature/wp6-codex-opencode |
| Started | 2026-11-02 03:46 |
| ETA (first) | 05:00 (≈ 75 min) |
| ETA (current) | done 04:15 |
| Phase | done |
| Progress | 7 / 7 tasks · 100 % |
| Last update | 2026-11-02 04:15 |

## Tasks
- [x] OSC 7501 scanner + parser + adapter (`osc7501.rs`)
- [x] Screen adapter + TOML manifest loader (bundled, user override dir, no fetch)
- [x] Codex manifest + OpenCode manifest fallback
- [x] OpenCode plugin + adapter
- [x] Process identity (`identify_agent`, `is_shell`, process observations)
- [x] Fixtures + replay tests (Codex recorded + authored, OpenCode, OSC 7501)
- [x] Review (round 1 CHANGES: 4 MUST-FIX + 4 NICE, all fixed; delta review APPROVE), parity, report

## Log
- 03:46 plan — started, ETA 05:00
- 03:55 implement — read plan, adapter contract, Orca OSC 7501 + OpenCode plugin, herdr manifests
- 04:00 implement — found Orca's real recorded Codex 0.155.1/0.157.1/0.158.0 PTY transcripts; built a mini VT for replay
- 04:05 test — fmt, clippy -D warnings, `cargo test --workspace` green
- 04:08 review — fresh reviewer (sonnet): CHANGES (unbounded OSC records, `clear` semantics, Codex status-row regex false Run, OpenCode 30 min expiry vs dedupe, 4 minor)
- 04:14 review — all fixed with tests; delta review APPROVE
- 04:15 done — parity table written; Playwright blocks file:// so no PNGs (see report)

## Handoffs and bounce-backs
| Time | Type | From → To | Reason | Resolved |
|---|---|---|---|---|
| 04:12 | bounce-back | reviewer → WP6 | CHANGES (4 MUST-FIX) | 04:14, delta APPROVE |

## Blockers / requests for shared files
See Requests below.

## Final report

**Files (all uncommitted in the worktree; I did not commit):** Rust src 3 modified + 1 new (`screen/manifest.rs`); tests 3 new files (`tests/{transcript_fixtures,opencode_fixtures}.rs`, `tests/support/mod.rs`); integrations: 2 manifests + 2 READMEs + 1 plugin + 10 authored/3 JSON fixtures + 6 recorded transcripts (`integrations/manifests/fixtures/recorded/`, ~200 KB). Nothing outside owns-paths.

**Done**
- `osc7501.rs`: streaming scanner (split-chunk safe, 4096 cap, cancel handling), parser ported from Orca's `protocol.ts`, `QUERY_REPLY`/`is_query`, adapter (root + sub-agent records, caps, clear semantics).
- `screen.rs` + `screen/manifest.rs`: TOML manifest format (regions, matchers, priorities, `keep`, summary capture), bundled + user dir (`$LOTSE_DATA_DIR/manifests`, else `~/Library/Application Support/Lotse/manifests`), strict parsing, size/regex/nesting limits, no fetch. Adapter: working→Run, blocked→Perm(+request), rest→Done after `hold_ms` (Idle if nothing ran), Process source (exit code, shell took foreground). `identify_agent`, `is_shell`.
- `opencode.rs` + `integrations/opencode/lotse-status.js`: payload contract, per-session tracking, children = sub-agents, ordered delivery queue, keep-alive, inert outside Lotse, no conversation text.
- Manifests `codex.toml` (working row, footer spinner, approval/trust/update dialogs, reconnect-failed, transcript viewer) and `opencode.toml` (fallback).

**Tests (quoted)**: `cargo fmt --all --check` clean; `cargo clippy --workspace --all-targets -- -D warnings` clean; `cargo test --workspace` green (before review fixes); after fixes `cargo test -p lotse-detect`: 53 unit + 3 claude + 3 opencode_fixtures + 5 transcript_fixtures pass, 1 ignored (exporter). Real recorded Codex transcripts (0.155.1, 0.157.1 ready, 0.158.0 ×4) replay to: approval → Perm("command", "touch approved.txt"), trust → Perm, update dialog → Perm, ready → Idle, two full tool turns → [Idle, Run, Done]. OpenCode plugin run under real `node` with a fake `lotse-hook` sends exactly the documented payloads (3 fixtures).

**Parity (non-UI; vs spec/mock.html data model, `P` at lines 289-293, rank at 304, labels at 537/660)**
| Feature | Mock | App | Match |
|---|---|---|---|
| Status names, run/perm/fail/done/idle | `run perm fail done idle` | engine `Status` from all adapters; `unknown` = no signal | match |
| Codex idle (a2) | idle | Idle on ready composer (recorded) | match |
| Codex fail (a4) | fail | `reconnect_failed` rule, non-zero exit (process); other Codex error screens not matched on purpose | partial gap, accepted: unverified error phrases, avoids false Fail on Esc interrupt |
| OpenCode done, unread (a6) | done | Done kept at rest until next work (plugin + screen) | match |
| Sub-agents done/run/perm (a1, a3) | per-sub status | OpenCode child sessions, OSC 7501 child records; Codex has none (mock: none) | match |
| Perm card tool + summary | Needs permission + command | Codex `$ cmd` captured, OpenCode tool/patterns, OSC kind/msg | match (OpenCode fields unverified) |
| Elapsed time of run (`el`) | seconds | `running_since` from engine on Run | match |
| Launch names `codex`, `opencode` | defaults | `identify_agent` uses the same names | match |
| Screenshots in `parity/wp6/` | n/a for non-UI | **none**: Playwright blocks `file://` and no server may be started (only :1421 for the app) | gap, accepted: no UI in this WP |

**Requests (shared files, not done by me)**
1. `crates/lotsed` (WP1/WP8 glue): per pane feed PTY output to `Osc7501Scanner::feed` → `Observation::Osc`; answer `osc7501::is_query` payloads by writing `osc7501::QUERY_REPLY` to the PTY; sample `Observation::Screen` after ~100 ms quiet (≤ every 500 ms streaming) from `alacritty_terminal`, **not inside a synchronized update (`?2026`)**; `Observation::Tick` ≈ 1 s; `Observation::Process` on foreground-pgrp change (`tcgetpgrp` + process name) and on child exit with the code; recreate/choose the Engine agent via `screen::identify_agent`; set `LOTSE_SESSION_ID` and `LOTSE_HOOK_BIN` (path to `lotse-hook`) in pane env.
2. `crates/lotse-hook` (WP2 owner): `lotse-hook integration opencode install|uninstall|status` writing `integrations/opencode/lotse-status.js` to `${XDG_CONFIG_HOME:-~/.config}/opencode/plugins/lotse-status.js` (embed the file); `lotse-hook opencode` already forwards stdin payloads.
3. No Cargo.toml change needed (`toml`, `regex`, `thiserror`, `tracing`, `tempfile` were already declared).

**Open questions**: none blocking. Is a shipped sample user manifest wanted in Settings?

**Unverified (labelled in code/READMEs)**: OSC 7501 spec page (superlogical.com) not read — grammar from Orca's reader; which agents emit it (none of Orca's Codex recordings do); OpenCode plugin docs and `permission.asked` / `question.asked` / `session.error` fields, plugin directory loading (Orca-derived only); OpenCode screen phrases (no recording; fixture authored); Codex versions beyond 0.155–0.160; `alacritty_terminal` honoring `?2026` and matching my test mini-VT; recorded `expect_sequence` values were generated by the engine after I hand-checked them against the recordings' meta notes and screens (not an independent oracle); `codex`/`opencode` CLIs were not run (not permitted here). Plan/spec gaps: ALPHA_PLAN says "OSC 7501 parser fed from the PTY stream"; the contract only has `OscObservation`, so the scanner lives in `osc7501.rs` and the daemon must call it (request 1).
