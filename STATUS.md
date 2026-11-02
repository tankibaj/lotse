# Lotse Alpha status

Orchestrator ledger and the truth after compaction. Plan: [ALPHA_PLAN.md](ALPHA_PLAN.md). Spec: [spec/](spec/). Live per-WP progress written by the agents: [status/](status/) (one file per WP).

## Work packages

| WP | Title | State | Phase · progress | ETA | Session time | Cost | Handoffs / bounce-backs | Verified | Mock parity | Merge |
|---|---|---|---|---|---|---|---|---|---|---|
| WP0 | Foundation and contracts | merged | done · 9/9 | 02:12 | 81m | $5.10 | 0 / 1 | CI set on main green (cargo 23 pass/3 ign; vitest 24; lint/typecheck/build ✓) | match (accepted: page window frame, Beta right toolbar; placeholders by design) | 9d8374e |
| WP1 | Daemon core | running | launched 03:10 · `wp1-daemon` | ≤04:40 | | | | | | |
| WP2 | Status engine + Claude detection | merged | done | 03:30 | ~22m | $4.22 | 0 / 1 | Rust CI on worktree + full CI set on main green (detect 17+3, hook 30+4, proto 22, lotsed 1; vitest 24) | match from mock source (non-UI; no screenshots: Playwright MCP blocks file://) | 50acff6 |
| WP3 | App shell, sidebar, status bar | running | launched 03:10 · `wp3-sidebar` | ≤04:40 | | | | | | |
| WP4 | Terminal panes | running | launched 03:10 · `wp4-terminal` | ≤04:40 | | | | | | |
| WP5 | Sheets, Settings, palette | running | launched 03:35 · `wp5-sheets` | ≤05:05 | | | | | | |
| WP6 | Codex + OpenCode detection | pending | | | | | | | | |
| WP7 | Git, notifications, native | pending | | | | | | | | |
| WP8 | Integration | pending | | | | | | | | |
| WP9 | Hardening + Alpha build | pending | | | | | | | | |

States: pending · running · review · merged · blocked. Branch, worktree and run dir live in each `status/WPn.md`.

## Delivery metrics

| Metric | Value | Notes |
|---|---|---|
| Session time (Alpha total) | 1h 43m | wall-clock of all runs; per WP in the table |
| Merges into `main` | 2 | |
| Failed changes | 0 | merge needed a fix or revert |
| Change failure rate | 0 % | failed ÷ merges; target ≤ 15 % |
| Escaped bugs | 0 | found after merge; see table below |
| Handoffs / bounce-backs | 1 / 2 | |
| Cost (Alpha total) | $9.32 | |

## Escaped bugs

| Found | By | Origin WP | Severity | Description | Fix commit |
|---|---|---|---|---|---|

## Handoffs and bounce-backs

| Time | WP | Type | From → To | Reason | Resolved |
|---|---|---|---|---|---|
| 02:04 | WP0 | bounce-back | reviewer → WP0 | harnex review CHANGES (5 must-fix) | 02:10 |
| ~03:25 | WP2 | bounce-back | reviewer → WP2 | CHANGES: stuck sub-agent, Stop overwrote fail, tool_response forwarded | yes |
| 03:30 | WP2 | handoff | WP2 → WP1/WP8 | lotsed: LOTSE_SESSION_ID env, HookEvent → engine, Process-exit + Tick observations; installer wiring | open |

## Mock parity

Per WP: feature · mock · app · match / gap (fixed, or accepted with a reason). Screenshots in `parity/<wp>/`.

**WP0** (orchestrator re-checked `parity/wp0/*-1440x900-dark.png`): sidebar 260 px, title row, search, PROJECTS header, tab row 44, status bar 28, fonts, light+dark tokens, theme key, ⌃⌘S hide → match. Accepted: mock's page-window frame (app fills viewport, native lights in Tauri); right-panel toolbar (Beta). Gap by design: tree/tabs/terminal/footer data (WP3/WP4), sheets/settings/palette (WP5).

## Requests from WPs (shared files, proto changes)

- WP0: optional cleanup of generated ignored icons / `.playwright-mcp/` (gitignored, harmless). Additive contracts accepted as frozen: `lib/ui/commands.ts`, `lib/ui/selection.ts`, `DaemonClient.onConnection`, `MAX_HELLO_LEN` + `*_max` readers, `default_adapter` stubs, `rust-toolchain.toml` 1.95.0. `@xterm/addon-canvas` not pinned (incompatible with xterm 6; WebGL + DOM renderer remain). `npm audit`: 11 moderate dev-only advisories, untriaged → WP9.
- WP2 → lotsed (WP1 follow-up or WP8): export `LOTSE_SESSION_ID` on spawned PTYs; route `HookEvent` into the engine; emit Process-exit and periodic Tick observations (sub-agent expiry). WP8/WP5: call the installer (`lotse_hook::claude_settings` or `lotse-hook integration claude install`). Proto comment fix: `HookEvent` no longer verbatim (`tool_response` stripped).
- **Decision for Naim (WP2):** `PostToolUseFailure` → `Run` (only `StopFailure` = red card); research.html lists it as a 'failed' source. One arm in `handle_hook` to flip. Open minors: parallel `PreToolUse` can mask an open perm; uninstall leaves backup (and `{}` if no settings existed). **Unverified:** hook names/fields checked against Orca/herdr local code only, not live docs.

## Log

- 2026-11-02 · Plan drafted, spec copied into `spec/` (mock sha256 `b221481f1d1b…`).
- 2026-11-02 · Plan approved by Naim. Defaults: $15 / 90 min per run, push `main` after each verified merge, worktrees removed after merge.
- 2026-11-02 01:40 · Toolchain: rust 1.95, node 26.5, Xcode CLT, swift 6.4, gh (tankibaj). `sccache` 0.19 installed; `RUSTC_WRAPPER=sccache` added to `~/.zshrc.local` (Naim OK'd).
- 2026-11-02 01:42 · `git init` (main), first commit `a97a85f chore: add alpha plan and spec`, private repo `tankibaj/lotse` created and pushed.
- 2026-11-02 01:46 · WP0 launched: worktree `lotse-wt/wp0-foundation`, branch `feature/wp0-foundation`, run dir `scratchpad/delegate/wp0-foundation-0145`. Deviation: `DELEGATE_ALLOW` widened beyond `Bash(date:*)` because the delegate allowlist has no `cargo`: cargo build/check/test/fmt/clippy/run/tree/metadata/generate-lockfile/add/update, npx playwright/tauri/vitest/eslint, and Edit/Write on `lotse/status/**` (tests the out-of-worktree status write).
- 2026-11-02 01:50 · Naim: urgent, finish by morning; checkpoints show-and-continue; keep workers running through orchestrator limits; parallelize. Heartbeat cron 6d2adece (:13/:43) resumes the runbook. Parallel cap raised 3 → 4 (12 cores / 34 GB, sccache); WP0 stays serial (hard dependency of all WPs). First-run check: delegated CLI CAN write `lotse/status/WP0.md` outside its worktree (with Edit/Write allow) → no fallback needed.
- 2026-11-02 01:55 · `/delegate` now detaches runs by default (survive orchestrator death/cap/app quit). Rule: orchestrator never stops worker runs; only Naim does. WP0 (launched 01:46) predates it and is still tied to this session.
- 2026-11-02 01:58 · Durable fallback: desktop scheduled task `lotse-orchestrator-fallback` (:27/:57, Auto mode, fresh session) resumes the runbook only if `lotse-wt/_orch/alive` is older than 40 min; this session touches it on every wake-up (heartbeat cron :13/:43). Orchestrator files now durable in `lotse-wt/_orch/` (mkbrief.sh, extras, `runs/` for new run dirs). WP0 run dir is still in this session's scratchpad: `/private/tmp/claude-501/-Users-naim-Repos-incubator-lotse/466dee2e-988b-4d15-a606-37208420c880/scratchpad/delegate/wp0-foundation-0145`.
- 2026-11-02 03:05 · Fallback orchestrator took over (alive file 56 min stale; main session died ~02:10 after WP0 CI). WP0 run `wp0-foundation-0145` done exit 0, $5.10, session 3a971f58.
- 2026-11-02 03:08 · WP0 verified in worktree (full CI set green), committed `167e3c8 feat(foundation): …`, merged `--no-ff` → `9d8374e`, full CI set on main green, pushed. Worktree removed, branch deleted. Parity re-checked from screenshots.
- 2026-11-02 03:08 · **Checkpoint 2 (show-and-continue):** empty shell vs mock in `parity/wp0/`; frozen contracts listed in WP0 final report (rolled into git history via `status/WP0.md`). Not waiting.
- 2026-11-02 03:08 · WP0 added `.github/workflows/ci.yml` (contents: read, no secrets) → suggest `/code-review` on CI before Beta.
- 2026-11-02 03:10 · Wave 1 + first wave-2 slot launched (4 parallel, all `harnex`, $15/90 min): WP1 `lotse-wt/wp1-daemon` `feature/wp1-daemon` run `_orch/runs/wp1-daemon-0310` port 1423 · WP2 `lotse-wt/wp2-status-claude` `feature/wp2-status-claude` run `_orch/runs/wp2-status-claude-0310` port 1424 · WP3 `lotse-wt/wp3-sidebar` `feature/wp3-sidebar` run `_orch/runs/wp3-sidebar-0310` port 1421 · WP4 `lotse-wt/wp4-terminal` `feature/wp4-terminal` run `_orch/runs/wp4-terminal-0310` port 1422. Extra brief notes: `_orch/wave1-extra.md` (contracts frozen, no web fetch → label Unverified). Waiters belong to fallback session 731ae749; runs are detached.
- 2026-11-02 03:32 · WP2 run done exit 0, $4.22, session a75dd747. Owns-paths respected (detect engine/claude + tests, lotse-hook, integrations/claude). Tests use env_clear + temp HOME + `--config-dir` (never real `~/.claude`). Committed `2a63918`, merged → `50acff6`, full CI set on main green, pushed, worktree + branch removed. WP2 touches Claude settings installer (security-relevant) → suggest `/code-review`.
- 2026-11-02 03:35 · WP5 launched into freed slot: `lotse-wt/wp5-sheets` `feature/wp5-sheets` run `_orch/runs/wp5-sheets-0335` port 1424 (from main 50acff6). Running: WP1, WP3, WP4, WP5. Next: WP6, WP7 when slots free.
