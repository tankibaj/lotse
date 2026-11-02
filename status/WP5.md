# WP5 · Sheets, Settings, palette

<!-- Written by the delegated agent only. One writer per file. -->

| Field | Value |
|---|---|
| Agent / run | claude CLI (harnex, headless) · wp5-sheets |
| Worktree / branch | ~/Repos/incubator/lotse-wt/wp5-sheets · feature/wp5-sheets |
| Started | 2026-11-02 03:36 |
| ETA (first) | 05:00 (≈ 85 min) |
| ETA (current) | done 03:59 |
| Phase | done |
| Progress | 10 / 10 tasks · 100 % |
| Last update | 2026-11-02 03:59 |

## Tasks
- [x] overlay store + shared Modal, hostBridge, symbols, validation (pure)
- [x] Add project sheet (browse, clone, create, drop, multi-repo)
- [x] New session sheet
- [x] Project colour/icon picker
- [x] Shortcuts sheet + ⌘T ⇧⌘O ⌘/ bindings
- [x] Settings page (Agents, Notifications, Appearance, Remote, Privacy, Shortcuts) + ⌘,
- [x] Palette ⌘K
- [x] Vitest component tests (142 pass, full suite)
- [x] Playwright walk (light/dark, 12/12) + parity screenshots + table
- [x] harnex gates: validator PASS, reviewer CHANGES → fixed → delta re-check CHANGES (missing test) → fixed, UI review CHANGES → fixed

## Log (newest last, `HH:MM phase — note`)
- 03:36 plan — read plan, spec, contracts; starting implementation
- 03:44 test — lint, typecheck, vitest 131, build green
- 03:49 test — Playwright walk 12/12 via installed Chrome (Playwright's own browser not downloaded; no network)
- 03:52 parity — 44 screenshots (mock vs app, 1440×900 and 1280×800, light/dark) in parity/wp5/
- 03:55 review — validator PASS; reviewer 3 MUST-FIX, UI review 3 MUST-FIX (+ contrast)
- 03:57 fix — all MUST-FIX fixed with tests; round 2: late-close guard tests added
- 03:59 done — lint, typecheck, vitest 142/142, build, Playwright 12/12 green

## Handoffs and bounce-backs
| Time | Type | From → To | Reason | Resolved |
|---|---|---|---|---|
| 03:55 | bounce-back | reviewer → WP5 | dup repos from preset list; worktree blocks non-git; Settings no focus | yes, tests added |
| 03:55 | bounce-back | UI reviewer → WP5 | focus lost on Add view change; shortcuts Tab leak + clip at 900×560 | yes, tests added |
| 03:58 | bounce-back | delta reviewer → WP5 | no test for late-close guard | yes |
| 03:59 | handoff | WP5 → orchestrator | Requests below | open |

## Blockers / requests for shared files
See Final report › Requests.

## Final report

**Files** (all under `app/src/{sheets,settings,palette}/`, uncommitted; plus `parity/wp5/` 44 PNG + 1 stray `probe.png` I could not delete, remove it): sheets 10 source + 5 test + 3 e2e; settings 4 source + 1 test; palette 3 source + 1 test. 4 existing placeholder files rewritten (`Sheets.tsx`, `SettingsPage.tsx`, `Palette.tsx`, `sheets/index.ts`). No change outside owns-paths; no dependency/lockfile change.

**Done**
- Add project: Browse (Tauri dialog; path field fallback in browser), Clone URL, Create new (+ optional git init), drag folder onto sidebar, multi-repo folder lists each repo (already-added ones disabled).
- New session: project, Terminal default, 4 agents, command hint with "Change…", optional worktree + branch validation, first prompt for agents.
- Colour/icon popover: 15 swatches, searchable symbols (41), each pick persisted via `updateProject`.
- Settings: Agents (commands, Reset, detected version/integration, empty → default), Notifications (all toggles saved), Appearance (theme, both fonts and sizes, applied + preview), Remote (disabled, Off), Privacy, Shortcuts.
- ⌘K palette (sessions by urgency, commands, projects), ⌘/ sheet, ⌘T, ⇧⌘O, ⌘, bindings; one overlay at a time; focus trap and return.

**Tests** (run by me after the last change): `npm run lint` clean · `npm run typecheck` clean · `npx vitest run` 12 files / 142 tests pass (WP5: 7 files, 118) · `npm run build` ok · Playwright walk `npx playwright test -c src/sheets/e2e/playwright.chrome.config.ts` 12/12 (6 flows × light/dark). Validator (earlier state of the code): PASS, same commands green, 131 tests, 12/12.

**Parity** (mock vs app, 1440×900 + 1280×800, light + dark, `parity/wp5/{mock,app}-<view>-<size>-<theme>.png`)
| Feature | Mock | App | Match |
|---|---|---|---|
| New session sheet | 440 px card, Run tiles, hint, worktree | same | match |
| Worktree branch / first prompt | shown on tick / for agents | same | match |
| Add project home / path / clone / create | 520 px, 3 rows, tip | same | match |
| Multi-repo list | not in mock UI | checkbox list | addition (plan scope) |
| Colour/icon popover | 8-col swatches, search, grid | same, 41 symbols | match |
| Settings Agents / Notifications / Appearance / Remote / Privacy / Shortcuts | 6 sections | same | match; Remote checkbox disabled (accepted: no listeners in Alpha) |
| Palette | sessions, then commands | sessions, commands, projects | match + projects; Split/Diff/Changes entries omitted (Beta/WP4) |
| Shortcuts sheet | no close button | Close ✕ added | accepted deviation (focus trap, a11y) |
| Select focus ring on open | none | autofocus ring | accepted (a11y) |
| Window frame offset | 32 px gradient frame | full window | WP0 shell, not WP5 |

**Requests** (shared files)
1. Sidebar (WP3): project menu "Icon & colour…" → `openProjectStylePicker(projectId, {left,bottom})` from `app/src/sheets`; project tiles can use `ProjectIcon`, `swatchColor` from the same index. Mock "Settings"/"Pause" menus likewise via `openSettings(section)`.
2. WP7/WP8: Tauri commands + capability for `hostBridge.ts` (PROPOSED names, **Unverified**): `scan_repos(path) -> {is_repo, repos[]}`, `clone_repo(url, dest)` (pass `--`), `create_project_dir(path, gitInit)`, and `dialog:allow-open` in `app/src-tauri/capabilities/default.json` + plugin registration. Daemon/host must expand `~/`.
3. WP4/WP8: terminal reads CSS vars `--lotse-terminal-font` / `--lotse-terminal-font-size` set on `<html>` by Settings (saved fonts applied at start).
4. WP8: theme is local-pref only (`theme/`); Settings writes `appearance.theme` to the daemon but does not read it back. Reconcile at integration.
5. Tokens (WP0): light-theme `green`/`orange` text on white ≈2.2:1 and blue-on-tint selected states 2.6–3.5:1 (same as mock; UI reviewer flagged). Consider darker text tokens.
6. Optionally `app/playwright.config.ts` (port 1421) vs my `src/sheets/e2e/playwright.config.ts` (port 1424, `*.chrome.config.ts` for installed Chrome).

**Open questions / decisions taken** (headless)
- Clone/Create/scan/native dialog live behind `hostBridge` (frozen client has no such calls): simulated in the browser, `invoke` in Tauri.
- Errors show after the first submit attempt, then live.
- Dev-only hook `window.__lotse.openProjectStylePicker` (import.meta.env.DEV) lets e2e open the popover until the sidebar does.
- Not fixed (NICE-TO-HAVE): failed folder drop shows no message; `~/x` vs `/Users/…/x` duplicate not detected; "Change…" leaves the New-session sheet; picker has ~100 tab stops; Settings nav uses `aria-pressed` not `aria-current`.

**Unverified**: real Tauri paths (native dialog, drag-drop physical→logical coordinates, `invoke` commands) — only through injected/simulated bridge; SF Symbols (Lucide used); Playwright's bundled browser not installed (used system Chrome).
