# WP4 · Terminal panes

<!-- Written by the delegated agent only. One writer per file. Absolute path: ~/Repos/incubator/lotse/status/WP4.md -->

| Field | Value |
|---|---|
| Agent / run | claude CLI (headless, harnex) · wp4-terminal · session n/a |
| Worktree / branch | ~/Repos/incubator/lotse-wt/wp4-terminal · feature/wp4-terminal |
| Started | 2026-11-02 03:11 |
| ETA (first) | 05:00 (≈ 110 min) |
| ETA (current) | done 03:42 |
| Phase | done |
| Progress | 8 / 8 tasks · 100 % |
| Last update | 2026-11-02 03:42 |

## Tasks
- [x] Read mock panes/tabs/split behaviour
- [x] Pane layout model + tests (add/close/focus/resize, max 4)
- [x] TerminalView (xterm + WebGL/DOM fallback, fit, attach/replay, input, resize, links, font)
- [x] Panes (splits, dividers, shortcuts, drag-drop) + TabStrip (tabs per project)
- [x] Font settings + ⌘+/⌘−/⌘0
- [x] Playwright e2e (typing round-trip, split/resize) + parity screenshots (parity/wp4/)
- [x] Validator + reviewer + UI/UX gates (3 rounds, all fixed)
- [x] Final report

## Log (newest last, `HH:MM phase — note`)
- 03:12 plan — started, read CLAUDE.md, plan, contracts, MockClient. ETA 05:00
- 03:16 implement — layout model, TerminalView, Panes, TabStrip written; typecheck + lint clean
- 03:19 test — vitest all green; pane/terminal tests added
- 03:22 test — Playwright e2e against dev server :1422 (installed Chrome; Playwright's own Chromium is not downloaded)
- 03:25 parity — 16 screenshots in parity/wp4/ (mock vs app); WebGL at 2x not verifiable headless
- 03:27 review — build ok; validator PASS (r1); reviewer CHANGES (3 MUST-FIX: key layout, settings failure, input after reconnect)
- 03:33 review — r2: fixes + tests; reviewer APPROVE; validator PASS
- 03:36 review — UI/UX reviewer CHANGES (3 MUST-FIX: tab arrow focus, Tab trap, focus after ✕ close)
- 03:41 review — r3: fixes + unit + e2e tests; reviewer APPROVE; full vitest 110/110, lint, typecheck, build, e2e 26/26
- 03:42 done — final report written

## Handoffs and bounce-backs
| Time | Type | From → To | Reason | Resolved |
|---|---|---|---|---|
| 03:30 | bounce-back | reviewer → me | shortcuts matched `e.code` (German layout), settings failure left panes blank, input dead after failed first attach + reconnect | yes, 03:33, tests added, reviewer APPROVE |
| 03:39 | bounce-back | UI/UX reviewer → me | arrow keys lose focus to terminal, Tab trap in xterm, focus lost after ✕ on an unfocused pane | yes, 03:41, unit + e2e tests, reviewer APPROVE |

## Blockers / requests for shared files
See Final report › Requests.

## Final report

### Files (all inside owns-paths; no `shell/` edit was needed, `TabStrip` mounts in the existing tabs slot)
- `app/src/panes/`: 7 source (`Panes`, `TabStrip`, `index` changed; `layout`, `splitBus`, `useSessions`, `StatusGlyph`, `ProjectTile` new) + 3 test files (`layout`, `Panes`, `TabStrip`)
- `app/src/terminal/`: 7 source (`TerminalView` changed; `font`, `links`, `renderer`, `focus`, `useTerminalAppearance`, `xtermTheme` new) + `fakeXterm.ts` (test double) + 3 test files (`TerminalView`, `font`, `focus`)
- `app/e2e/`: `wp4-terminal.spec.ts` (10 tests), `wp4-parity.spec.ts` (16 screenshot captures)
- `parity/wp4/`: 16 PNGs. Nothing outside owns-paths changed (no package.json / lockfile / proto / shell / contract edits).

### Done
- xterm.js 6 terminal per session: WebGL renderer (DOM renderer fallback), fit + ResizeObserver, attach/replay through `DaemonClient` (subscribe first, attach one tick later so StrictMode never double-replays), keystrokes/binary to `writePty`, resize to daemon, re-attach + `reset` on `reconnected`, detach on unmount, ⌘-click links (http/https only), light/dark/auto via theme tokens, font family/size from `settings.appearance.terminal_font*`, ⌘+ ⌘− ⌘0 saved through `updateSettings` (Settings page sees the same value), 9 to 32 px.
- Tabs per project (tablist, status glyph, project tile, arrow keys), "+" raises `openNewSession`.
- Splits up to 4 side by side: ⌘D (most urgent unopened session), `openInSplit(id)` (card context menu / palette), ⌥-click on any `[data-session-id]` element, drop of `text/lotse-session` with the mock's "Open side by side" hint; resizable dividers (pointer + arrow keys, 220 px minimum); ⌘[ ⌘] focus (wrapping); ⌘W closes the focused pane (session keeps running; last pane stays); pane header with focus accent, dimming, ✕.
- Keyboard way out of the terminal: Ctrl+Tab / Ctrl+Shift+Tab (xterm swallows Tab).
- Shortcuts follow the typed character, not the physical key (German layout works).

### Tests (numbers are from my last run after the final change; validator round 2 confirmed 102/102 before the last small UI-fix round, reviewers confirmed 78/78 and 85/85 on the pane/terminal subset)
- `npx vitest run`: 11 files, 110 tests, 0 failed (WP4's part: 6 files in `src/panes` + `src/terminal`, 85 tests; the rest is WP0's)
- `npm run lint`, `npm run typecheck`, `npm run build`: clean (build warns only: 715 kB chunk)
- Playwright against `npm run dev -- --port 1422` (MockClient): `e2e/wp4-terminal.spec.ts` 10/10 (typing round-trip, tabs, keyboard exit, ✕ focus, ⌘D/⌘[/⌘]/⌘W + 4-pane cap, divider drag + minimum, card drop, ⌘+/⌘−/⌘0, theme, WebGL mounted); full `e2e` 26/26 including 16 parity captures
- Gates: validator PASS (r1, r2), reviewer CHANGES → APPROVE (r2, r3), UI/UX reviewer CHANGES → fixed, covered by tests, delta reviewer APPROVE. No Rust touched, no dependency change, secret scan (grep fallback) clean.

### Parity (mock = `spec/mock.html`, app = `npm run dev` + MockClient; shots in `parity/wp4/<viewport>-<state>-<theme>-<mock|app>.png`, 1440×900 single/split2/split4 and 1280×800 split2, light + dark; `<main>` only, sidebar/status bar are WP3)
| Feature | Mock | App | Match |
|---|---|---|---|
| Tab strip: status glyph, title, project tile + name, active/inactive style, "+" | yes | same | match |
| Tab row panel buttons (Session/Changes/Files/Transcript/Usage) | shown | absent | accepted: Beta, out of Alpha scope |
| Single pane terminal look (colours, 20 px padding, dim/orange/green/red/blue) | HTML text | xterm theme from the same tokens | match (light + dark) |
| Row pitch | 19.5 px | ~19.5 to 20 px (xterm `lineHeight` 1.35, a glyph-height multiplier) | match within 0.5 px |
| Cell width | 7.8 px | DOM renderer 7.8 px; WebGL rounds cells to whole device pixels (7 px at 1x, ~7.5 at 2x) | accepted deviation (xterm WebGL) |
| Split header (tile, "title · project", status glyph, ✕, blue focus line) | yes | same | match |
| Unfocused pane dimmed 0.62 | yes | yes | match |
| Divider 1 px, ±6 px hit area, resize min 220 px | yes | yes (+ keyboard, aria-valuenow) | match |
| Up to 4 panes + message | "Up to 4 panes side by side" | same text, same toast look | match |
| Drag card onto panes → hint "Open side by side" | yes | same classes/text | match (synthetic drag in e2e; real cards = WP3) |
| Content overflow | no wrap, scrolls sideways | real terminal reflows at the pane's columns, PTY resized | accepted: a terminal wraps by design |
| Unfocused cursor | solid block | hollow block | accepted: iTerm2 behaviour |
| Box drawing (perm box) | text glyphs with gaps | continuous custom glyphs | accepted: xterm `customGlyphs`, looks better |
| Cursor position after replay | on prompt line | on line after the replayed text | accepted: MockClient screen data ends with `\r\n` |
| Scrollbar | none | quiet overlay slider | accepted (themed to be subtle) |
| Interactions walked: tab click, ⌘D, ⌘[ ⌘], ⌘W, ✕, divider drag + keys, drop hint, ⌘+/−/0, theme switch | n/a (mock has no ⌘D etc.) / click, ✕, drag, drop | all pass in e2e | match |

### Requests (shared files / other WPs; nothing applied by me)
1. **WP3 (sidebar):** session cards must carry `data-session-id="<id>"` (⌥-click split is a capture listener in `Panes`, it stops the card's own click), set `e.dataTransfer.setData('text/lotse-session', id)` on drag start, and call `openInSplit(id)` (exported from `app/src/panes`) from the card context menu "Open in Split Right". WP3 may also set the initial selection: `Panes` selects the first session if none is selected after the snapshot, so both can coexist.
2. **WP8 / orchestrator:** link opening uses `window.open(url, '_blank', 'noopener,noreferrer')` for http(s) only (`terminal/links.ts`); in the Tauri webview that may not reach the system browser. Needs `@tauri-apps/plugin-opener` (new pinned dependency + capability) or an equivalent Tauri command. Unverified there.
3. **Orchestrator (plan):** `ALPHA_PLAN.md` says "WebGL (canvas fallback)". `@xterm/addon-canvas` is not in the pinned deps and was removed upstream for xterm 6; the fallback is xterm's built-in DOM renderer. No change needed unless you want a different fallback. `localStorage['lotse-term-renderer']='dom'` forces it (diagnostic switch and what e2e uses to read text).
4. **CI note:** `app/playwright.config.ts` hard-codes `baseURL` 1421 and the Playwright browsers are not downloaded here; my specs take `LOTSE_E2E_URL` (default 1422) and fall back to installed Chrome. Not part of CI (as before).
5. `app/src/lib/state` (WP3) will probably duplicate `panes/useSessions.ts` (sessions/projects snapshot + events). Swap it for WP3's selectors when they land.

### Open questions / decisions taken (headless defaults)
- ⌘W with a single pane does nothing (shows "The last pane stays open"); closing a pane never closes the daemon session (mock behaviour).
- Line height 1.35 (xterm multiplier) instead of the CSS 1.5 so rows match the mock's 19.5 px.
- Link click needs ⌘ (or Ctrl), like iTerm2; plain click selects.

### Unverified
- WebGL at 2x (real Retina / WKWebView): Playwright's `deviceScaleFactor` emulation changes `window.devicePixelRatio` but the WebGL renderer drew at the screen's real ratio (mis-scaled text), so only 1x is checked headless (e2e "WebGL renderer is mounted and sized to the screen at 1x"); parity shots use the DOM renderer. Please look at `npm run tauri dev` on a Retina Mac once; if WebGL misbehaves there, set `lotse-term-renderer=dom` and tell me.
- Link opening, ⌘ key delivery, Ctrl+Tab and focus-on-click behaviour inside Tauri's WKWebView (only Chrome and jsdom were run). xterm docs to check: https://xtermjs.org/docs/ (custom key handler, WebGL addon).
- Red check on a base checkout (git worktree denied for the validator): new test files import modules that do not exist on base, so they fail there by construction; not executed.
- Real daemon (WP8): replay ordering (`onPtyData` before `attach` resolves) is assumed from the `DaemonClient` doc comment, MockClient only.
- Artifacts: `app/test-results/`, `app/.playwright-mcp/`, `.playwright-mcp/`, `app/dist/` are gitignored leftovers from test runs and the MCP browser.
