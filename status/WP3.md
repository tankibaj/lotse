# WP3 · App shell, sidebar, status bar

<!-- Written by the delegated agent only. One writer per file. Absolute path: ~/Repos/incubator/lotse/status/WPn.md -->

| Field | Value |
|---|---|
| Agent / run | claude CLI (harnex, headless) · wp3-sidebar |
| Worktree / branch | ~/Repos/incubator/lotse-wt/wp3-sidebar · feature/wp3-sidebar (uncommitted, orchestrator commits) |
| Started | 2026-11-02 03:11 |
| ETA (first) | 04:45 (≈ 95 min) |
| ETA (current) | done 03:45 (after parity bounce-back) |
| Phase | done |
| Progress | 8 / 8 tasks · 100 % |
| Last update | 2026-11-02 03:40 |

## Tasks
- [x] lib/state: store (snapshot + events, race-safe), selectors (roll-up, need-you, coarse time), selection init/repair, shortcuts, Dock-badge source
- [x] sidebar: tree, project row, session card (perm actions, retry swap, sub-agent fold), keyboard nav, need-you filter
- [x] sidebar: context menus (project, session), confirm dialog, inline rename, empty project
- [x] statusbar: need-you, git, daemon state, count, bell, LAN Off, tokens
- [x] Vitest: roll-up, filter, keyboard nav, truncation (+ store race, menus, shortcuts)
- [x] Playwright: resize, hide, 30-project scroll, light/dark screenshots
- [x] Parity gate (parity/wp3/) + harnex validator / reviewer / UI review (1 fix round)
- [x] Final report

## Log (newest last, `HH:MM phase — note`)
- 03:11 plan — started. Plan approved: skipped planner, flow = harnex (headless), WP implemented inline, then validator + reviewer + UI reviewer.
- 03:22 test — lint, typecheck, vitest 86/86, build, playwright 12/12 (installed Chrome via `channel: 'chrome'`).
- 03:25 parity — screenshots in parity/wp3 (app + mock, light/dark). Footer branch fixed to full name as in the mock.
- 03:27 review — validator: all green, red check NOT VERIFIED (cannot copy tests into a base checkout). Reviewer: APPROVE, 1 MUST-FIX (test gap: snapshot/event race) + nice-to-haves.
- 03:29 fix round 1 — added 4 race tests (mutation-checked: removing the replay line fails one), stable clock timer, ↑/↓ from project row, "Custom" no-op when already custom, ⌘J on typed letter.
- 03:35 UI review — CHANGES: 3 MUST-FIX. Fixed focus return after Allow/Retry/rename, perm buttons wrap at 200 px, Remove button contrast. Contrast of mock-inherited tokens (orange text, white on #007AFF, blue links) logged as accepted deviation.
- 03:40 done — final: lint clean, typecheck clean, vitest 96/96 (10 files), build ok, playwright 12/12.
- 03:41 bounce-back (orchestrator parity rejection) — agent label truncated before the branch on cards. Wrote failing tests first (Vitest class assertion + Playwright geometry at 260 and 200 px; both red on old code), then fixed.
- 03:45 done again — lint clean, typecheck clean, vitest 97/97 (10 files), build ok, playwright 13/13. Parity PNGs retaken.

## Handoffs and bounce-backs
| Time | Type | From → To | Reason | Resolved |
|---|---|---|---|---|
| 03:27 | bounce-back | reviewer → me | MUST-FIX: no tests for snapshot/event race in store.ts | yes, 03:29 |
| 03:35 | bounce-back | UI reviewer → me | MUST-FIX: focus lost after inline actions; perm buttons clipped at 200 px; contrast | yes (contrast of mock tokens: accepted deviation) |
| 03:41 | bounce-back | orchestrator → me | Parity gap: agent name truncated ("Claud…") while the branch kept width; mock truncates the branch | yes, 03:45 (see "Parity fix" below) |

## Blockers / requests for shared files
See "Requests" in the final report.

## Parity fix (bounce-back 03:41)
`SessionCard.tsx` agent · branch line: agent label `shrink-0 whitespace-nowrap`, separator and ↑/± counts `shrink-0`, only the branch `min-w-0 truncate` (full name in `title`). Card is an `@container`; below 170 px card width (sidebar ≈ 220 px and narrower) the branch icon and the separator margin drop out so agent and counts stay whole with no overflow at 200 px (the branch can shrink to "…"). Tests (red before, green after): `Sidebar.test.tsx` "agent · branch line: only the branch truncates…" and `e2e/wp3-sidebar.spec.ts` "agent · branch line: the branch truncates, the agent label and counts stay whole, at 260 and 200 px" (clip/width/overflow geometry per card). Retaken: `parity/wp3/{light,dark}-1440-default.png`, `light-1440-w200.png` (also the other e2e shots). Numbers now: vitest 97/97, playwright 13/13. Parity table row "Card layout" stays match; the 200 px note: icon hidden below ≈ 220 px sidebar (accepted).

## Final report

**Files (all uncommitted in the worktree):** `app/src/lib/state` 6 source + 3 test files · `app/src/sidebar` 11 source + 2 test/util files · `app/src/statusbar` 1 source + 1 test · `app/e2e` 2 specs · `parity/wp3` 17 PNG. Nothing outside the owns-paths (`git status` checked; no package.json/lockfile/shell/client/proto changes).

**Done**
- State (`lib/state`): store per client (snapshot + events, queue-and-replay while a snapshot loads, refetch on reconnect, StrictMode safe), pure selectors (roll-up perm>fail>run>done>idle, need-you, coarse age, filter, visible order, ⌘J), shared UI state (need filter, collapsed projects persisted in `lotse-collapsed`), `useCoarseNow` (re-renders only when a visible label changes, never under pointer/focus), `subscribeNeedYouCount` (Dock-badge source), initial selection + sibling repair + markRead, global shortcuts ⌘J, ⌘1…9, ⌥⌘N (mounted from the status bar, so they work with the sidebar hidden).
- Sidebar: Search + Projects header fixed, only the list scrolls; project row (tile, chevron on hover/focus, no-git/muted/custom badges, roll-up only when collapsed); session card (status glyph, title, unread dot, coarse time, agent · branch line, Allow/Always/Deny always visible on perm, Retry swap on fail, sub-agent fold, drag payload `text/lotse-session`); ↑/↓/Enter/→/←; need-you filter (+ banner, auto-off); project menu (New Session, Notifications global/custom/mute with per-event toggles, Copy Path, Rename, Remove with confirm) and session menu (Open, Rename, Stop with confirm when working; Split/Transcript/Changes shown disabled); empty-project row; ContextMenu key / Shift+F10.
- Status bar: need-you chip (filter toggle), git of current session (branch, worktree pill, ↑ahead, ±dirty, "No git"), daemon state + session count, bell (pause notifications via settings, ⌥⌘N), LAN Off, tokens `—`.
- Resize 200–420 / persistence / ⌃⌘S live in the shell (WP0) and are covered by the new Playwright run.

**Tests (numbers)**
- `npm run lint` clean · `npm run typecheck` clean · `npx vitest run` 10 files, 96 tests passed (5 new files: selectors, store, clock, Sidebar, StatusBar; 5 existing WP0 files unchanged) · `npm run build` ok (292.9 kB js) · `npx playwright test` 12/12 (10 sidebar/status + 2 mock screenshots) against `npm run dev -- --port 1421`.
- Cargo not touched (not run).
- Playwright uses installed Chrome (`test.use({ channel: 'chrome' })` in the specs): the bundled browser is not downloaded in this setup. `LOTSE_E2E_BUNDLED=1` switches back. Dev-only handle `window.__lotse.client` (set in `store.ts` under `import.meta.env.DEV`) lets the 30-project test seed the MockClient.
- Validator (harnex): PASS on lint/typecheck/vitest/build/playwright/manual walk; red check NOT VERIFIED (could not copy new test files into a base checkout, all new files are untracked so they cannot pass on base). I mutation-checked the race test myself (replay line removed → test fails).
- Reviewer (harnex, sonnet): APPROVE after fixing 1 MUST-FIX (race tests). UI/UX reviewer: CHANGES → 3 MUST-FIX fixed (no second UI delta pass run; verified by tests, w200 screenshot, full suite).

**Parity table** (mock = `spec/mock.html` via file:// in Chrome, app = `npm run dev` MockClient; 1440×900 and 1280×800, light and dark; screenshots `parity/wp3/`: `{light,dark}-1440-{default,collapsed}.png`, `{light,dark}-1280-filter.png`, `light-1440-w{200,420}.png`, `light-1440-30-projects.png`, `light-1440-retry-hover.png`, `dark-1440-project-menu.png`, mock side `mock-{light,dark}-1440-{default,collapsed}.png`)

| Feature | Mock | App | Match |
|---|---|---|---|
| Project tree, tiles, colours | per mock | same swatches/icons (lucide) | match |
| Card layout (title, time, agent · branch, ↑±, sub-agents) | 88 px selected card | 88 px (measured on screenshot) | match |
| Roll-up when collapsed (▲, ✕, top state, idle none) | yes | yes | match |
| Perm buttons always visible | yes | yes; wrap at 200 px | match (+ wrap) |
| Retry swap on fail, no height change | yes | yes (Playwright asserts height) | match |
| Chevron only on hover/focus | yes | yes | match |
| Need-you filter (footer chip, banner, Show all) | yes | yes, also auto-off at 0 | match |
| Context menus (project + Notifications submenu, session) | yes | yes; keyboard operable | match (disabled items below) |
| Resize 200–420, hide ⌃⌘S, persistence | yes | shell; Playwright verifies clamp, persist, hide | match |
| Footer: need-you, git (full branch), worktree pill, daemon, bell, LAN Off | yes | yes | match |
| Footer tokens | `412k tokens · $4.18` | `Today — tokens · —` | accepted: plan says `—` in Alpha |
| Footer daemon text | `lotsed running · 6 sessions` | `7 sessions` (fixture has 7) | accepted: mock text is hard-coded |
| Time label of a3 | `now` | `<1m` | accepted: spec says `<1m` |
| Default expanded state | infra, dotfiles collapsed | all expanded, user collapse persisted | accepted: proto has no `open` flag |
| Menu key hints ⌥⌘R / ⌥⌘C | shown | not shown (not bound) | accepted: no dead hints |
| Change Icon & Color, Reveal in Finder, Split Right | enabled | disabled, hint "Soon" | accepted: no UI command / native bridge yet (see Requests) |
| Show Transcript / Changes | n/a | disabled "Beta" | per plan decision |
| "▲ 0 need you" | orange | dim + disabled | accepted: calm when nothing needs you |
| Tabs, panes, window frame | mock has them | WP4 / shell placeholders | out of scope |
| Contrast: orange text (2.0:1), white on #007AFF (4.0:1), blue links | same tokens | same tokens | accepted deviation, inherited from the mock; token decision for Naim (UI reviewer measured). Remove button uses a darker red (not in mock) |

**Requests (shared files, not done)**
1. `app/src/lib/ui/commands.ts`: add `openProjectStyle` (payload project id) for WP5's icon/colour popover, and `splitSession` (payload session id) for WP4; and a payload (project id) on `openNewSession` so the sheet preselects the clicked project. Then the sidebar enables "Change Icon & Color…", "Split Right" and passes the project. Exact diff: `UiCommand` union += `'openProjectStyle' | 'splitSession'`; `emitUiCommand(command, arg?: string)`, handlers receive `arg`.
2. WP8/WP7: Dock badge: call `subscribeNeedYouCount(client, n => setBadgeCount(n))` from `lib/state` in the Tauri entry (needs the window badge capability in `src-tauri`). "Reveal in Finder": expose a Tauri command (opener) and wire the disabled menu item.
3. WP8: dropping a folder on the sidebar needs Tauri's drag-drop event (DOM drop gives no path); not implemented in the sidebar.
4. `app/playwright.config.ts` has no `projects`/`webServer`; my specs set `channel: 'chrome'` themselves. Orchestrator may prefer a config-level choice.
5. `lib/ui/selection.ts` comment says WP3 sets the initial selection: done in `lib/state/store.ts` (first session; sibling when closed).

**Open questions (defaults applied)**
- Default expanded vs the mock's two collapsed projects (default: all expanded).
- Contrast tokens (orange/blue on light) are the mock's; change at token level if wanted.
- ⌥⌘N matches the physical key (⌥ changes the typed character on macOS); ⌘J matches the typed letter. Non-QWERTY layouts **Unverified**: check in the built app.

**Unverified**
- Red check (new tests fail on base): inference only, plus one mutation check.
- Real-daemon behaviour: `session_exited`, `connected`/`reconnected` ordering (only MockClient exists). Docs/URL to check: `crates/lotse-proto` when WP1 lands.
- The `__lotse` dev handle and Chrome channel assume local dev; nothing runs in CI (`playwright` is not in the CI command set).
- No second UI-reviewer pass after the round-1 fixes.
- `harnex.mjs untracked save/new` was not run (baseline not saved at run start); stray files found: none in git status; `.playwright-mcp/` (UI reviewer screenshots) exists in the worktree root and is git-ignored.
