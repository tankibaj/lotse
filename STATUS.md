# Lotse Alpha status

Orchestrator ledger and the truth after compaction. Plan: [ALPHA_PLAN.md](ALPHA_PLAN.md). Spec: [spec/](spec/). Live per-WP progress written by the agents: [status/](status/) (one file per WP).

## Work packages

| WP | Title | State | Phase · progress | ETA | Session time | Cost | Handoffs / bounce-backs | Verified | Mock parity | Merge |
|---|---|---|---|---|---|---|---|---|---|---|
| WP0 | Foundation and contracts | pending | | | | | | | | |
| WP1 | Daemon core | pending | | | | | | | | |
| WP2 | Status engine + Claude detection | pending | | | | | | | | |
| WP3 | App shell, sidebar, status bar | pending | | | | | | | | |
| WP4 | Terminal panes | pending | | | | | | | | |
| WP5 | Sheets, Settings, palette | pending | | | | | | | | |
| WP6 | Codex + OpenCode detection | pending | | | | | | | | |
| WP7 | Git, notifications, native | pending | | | | | | | | |
| WP8 | Integration | pending | | | | | | | | |
| WP9 | Hardening + Alpha build | pending | | | | | | | | |

States: pending · running · review · merged · blocked. Branch, worktree and run dir live in each `status/WPn.md`.

## Delivery metrics

| Metric | Value | Notes |
|---|---|---|
| Session time (Alpha total) | 0h 00m | wall-clock of all runs; per WP in the table |
| Merges into `main` | 0 | |
| Failed changes | 0 | merge needed a fix or revert |
| Change failure rate | — | failed ÷ merges; target ≤ 15 % |
| Escaped bugs | 0 | found after merge; see table below |
| Handoffs / bounce-backs | 0 / 0 | |
| Cost (Alpha total) | $0.00 | |

## Escaped bugs

| Found | By | Origin WP | Severity | Description | Fix commit |
|---|---|---|---|---|---|

## Handoffs and bounce-backs

| Time | WP | Type | From → To | Reason | Resolved |
|---|---|---|---|---|---|

## Mock parity

Per WP: feature · mock · app · match / gap (fixed, or accepted with a reason). Screenshots in `parity/<wp>/`.

_None yet._

## Requests from WPs (shared files, proto changes)

_None yet._

## Log

- 2026-11-02 · Plan drafted, spec copied into `spec/` (mock sha256 `b221481f1d1b…`). Waiting for Naim's approval.
