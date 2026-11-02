# Lotse Alpha plan (Pilot MVP)

Status: **draft, waiting for Naim's approval** · Written 2026-11-02 · Owner: orchestrator session

Lotse is a macOS terminal app like iTerm2, with projects and agent awareness like Orca. **Terminal first:** the main view is the full-size agent CLI (Claude Code, Codex, OpenCode or a shell). Everything else stays out of the way.

## Spec (source of truth)

| File | What it settles |
|---|---|
| [`spec/mock.html`](spec/mock.html) | The final UI. Copy of `/Users/naim/Repos/research/agent-monitor/mocks-v3/00-quick-main.html` (2026-11-02, sha256 `b221481f1d1b…`). Open it in a browser; its JS data model (`P`, `G`, status names) is the reference for UI state. |
| [`spec/research.html`](spec/research.html) | Scope (Alpha / Beta / v1.x), security stance, what to take from Orca and herdr, sidebar and panel design rules (`#sidebar`, `#panels`), stack. Copy of `/Users/naim/Repos/research/agent-monitor/research.html` (2026-11-02); its mock links point to `spec/mock.html`. |

The copies in `spec/` are the build spec; the originals in `research/agent-monitor` are the design workspace. If the design changes there, re-copy and note it in `STATUS.md`. Orca and herdr source for reference: `/Users/naim/Repos/research/orca`, `/Users/naim/Repos/research/herdr`.

If the plan and the spec disagree, the spec wins for UI, the plan wins for structure. Alpha scope is `spec/research.html` › Scope › Release 1. **Right panels (Changes, Session, Transcript, Files, Usage), the editor pane, the Allow/Deny notification and LAN are Beta: out of Alpha.**

## Decisions (fixed for Alpha)

- **Repo:** `/Users/naim/Repos/incubator/lotse`, GitHub private `tankibaj/lotse` (created in WP0). `main` is the integration branch. WP branches `feature/<wp-slug>` stay local; the orchestrator pushes `main` after each verified merge.
- **Commits:** Conventional Commits `type(scope): summary`. **No `Co-Authored-By` trailers** (Naim's rule). The orchestrator commits; delegated agents never commit.
- **Stack:** Rust 1.95 workspace + Tauri 2 (React 19, TypeScript, Tailwind, xterm.js with the WebGL addon). macOS on Apple Silicon only.
- **Process model:** the `lotsed` daemon owns the PTYs and state. The app is a client and can quit or restart without killing sessions. The app starts `lotsed` (detached, `setsid`) when the socket is missing. Sessions do not need to survive a daemon restart or a reboot in Alpha.
- **Transport:** Unix socket `~/Library/Application Support/Lotse/lotsed.sock`, mode 0600, directory 0700. Length-prefixed frames: type `0x01` = JSON control message, type `0x02` = PTY bytes (session id + payload). Every client sends a token from `…/Lotse/token` (0600) first. **No TCP listener anywhere in Alpha.**
- **App ↔ webview:** Tauri commands for requests, Tauri events for state, `tauri::ipc::Channel` for PTY output. Strict CSP, no remote content, minimal capabilities.
- **Terminal state:** the daemon keeps a raw byte ring buffer per session (default 4 MiB), replayed to xterm.js on attach. It also feeds a VT model (`alacritty_terminal`) used only for screen-based detection.
- **Persistence:** SQLite (`rusqlite`, bundled) in the daemon: workspaces, projects (path, colour, icon), sessions (metadata, title, launch command), settings.
- **Detection order:** Claude Code hooks → OSC 7501 → screen scrape with TOML manifests (herdr style, bundled, no remote fetch) → process-group identity.
- **Status model:** `run | perm | fail | done | idle | unknown`, per session and per sub-agent. Roll-up order for collapsed projects: perm > fail > run > done > idle.
- **Tokens and cost in the status bar:** Alpha shows `—`. The transcript parser is Beta.
- **Workspaces:** the data model supports them. Alpha UI shows one default workspace; grouping UI comes later.
- **Session menu items Show Transcript / Show Changes:** shown disabled with a "Beta" hint.
- **SF Symbols:** Alpha renders symbols through a small Swift helper if WP7 lands it; otherwise Lucide stays as the fallback (as in the mock).
- **Telemetry:** none, ever. No update checks, no remote manifests.

## Repo layout (created in WP0)

```
lotse/
  Cargo.toml                 # workspace
  crates/
    lotse-proto/             # frames, messages, ids, status types, fixtures (WP0 owns, frozen after)
    lotsed/                  # daemon: socket, PTYs, registry, persistence, launch commands (WP1)
    lotse-detect/            # status engine + adapters: claude, osc7501, screen, opencode (WP2, WP6)
    lotse-hook/              # tiny CLI agents' hooks call; forwards JSON to the socket (WP2)
    lotse-git/               # branch, ahead, dirty, worktree create (WP7)
  integrations/
    claude/                  # hook config templates + installer data (WP2)
    opencode/                # OpenCode plugin (WP6)
    manifests/               # screen-scrape TOML manifests (WP6)
  native/symbols/            # Swift helper for SF Symbols (WP7, optional)
  app/
    package.json             # all Alpha npm deps pinned in WP0, incl. @tauri-apps/cli
    src-tauri/               # Tauri shell: daemon client bridge (WP0 skeleton, WP8 real wiring)
    src/
      lib/client/            # DaemonClient interface + MockClient (WP0) + real client (WP8)
      lib/state/             # store, selectors (roll-up, need-you) (WP3)
      theme/                 # tokens from the mock, light/dark/auto (WP0)
      shell/                 # window layout with slots: sidebar, tabs, panes, statusbar (WP0 skeleton)
      sidebar/  statusbar/   # WP3
      terminal/  panes/      # WP4
      sheets/  settings/  palette/   # WP5
  spec/                      # mock.html, research.html
  ALPHA_PLAN.md  STATUS.md  CLAUDE.md  README.md
```

## Work packages

Legend: **Mode** `harnex` = delegate run (Claude CLI, rw) with `/harnex <WP>` inside; `harnex quick` = `/harnex quick`. **Owns** = the only paths the WP may change (plus its own tests). Anything else goes in the report as a request.

### WP0 · Foundation and contracts (serial, first)
- **Goal:** everything the parallel WPs share, so they never need each other's code.
- **Scope:**
  - Cargo workspace with empty-but-compiling crates; npm app with Vite, React, Tailwind, xterm.js, Tauri 2; **all expected Alpha dependencies pinned now** (Rust and npm) so later WPs don't touch lockfiles.
  - `lotse-proto`: frame codec, every Alpha message (projects, sessions, attach/detach, input, resize, status events incl. sub-agents, settings, launch-command detection, git info), status enum, roll-up function, serde fixtures (`fixtures/*.json`). Versioned `PROTO_VERSION = 1`.
  - `lotse-detect`: the `Adapter` trait and the engine's public API (types only, `todo!()` bodies behind tests marked ignored). **Pre-declare every module** (`engine`, `claude`, `osc7501`, `screen`, `opencode`) as stub files so WP2 and WP6 never edit `lib.rs`.
  - App: pre-create every WP folder (`sidebar`, `statusbar`, `terminal`, `panes`, `sheets`, `settings`, `palette`) with an exported placeholder component already mounted in `shell/`, so UI WPs never edit `shell/` or routing.
  - App: theme tokens copied from the mock's CSS variables (light, dark, auto); `shell/` layout with named slots; `DaemonClient` TS interface mirroring the proto; `MockClient` seeded with the mock's data (projects, sessions, sub-agents, statuses) and scripted status changes.
  - `CLAUDE.md` for agents (commands, conventions, terminal-first rule, no network listeners, no telemetry, owns-paths rule, how to run the mock client); `README.md`; `STATUS.md`.
  - CI: GitHub Actions on `macos-14`: `cargo fmt --check`, `cargo clippy -D warnings`, `cargo test`, `npm run lint`, `npm run typecheck`, `npx vitest run`, `npm run build`.
  - Dev scripts: `npm run dev` (Vite with MockClient, port from `VITE_PORT`, default 1420), `npm run tauri dev`.
- **Done when:** CI green on `main`; `npm run dev` shows the empty shell in light and dark; proto round-trip tests pass for every fixture.
- **Mode:** harnex. **Then:** Naim demo checkpoint, contracts frozen (changes after this need an orchestrator-approved proto bump).

### WP1 · Daemon core
- **Owns:** `crates/lotsed/**`
- **Scope:** socket server (0600/0700, token handshake), multi-client; project and session registry; spawn/attach/detach/input/resize/kill via `portable-pty` with login-shell env; ring buffer replay; SQLite schema and migrations; launch commands with overrides, `$SHELL -l` default, PATH lookup and version detection (`claude --version` etc., timeout 3 s); session title from first prompt (editable); detached start, pidfile, graceful shutdown.
- **Done when:** integration tests drive the daemon over the socket: create project → spawn `/bin/sh` → write `echo hi` → read `hi` → detach → reattach replays it → kill; restart of a *client* keeps the session; socket and token file modes asserted; a test asserts no TCP listener (`lsof -iTCP -a -p <pid>` empty).
- **Mode:** harnex.

### WP2 · Status engine and Claude Code detection
- **Owns:** `crates/lotse-detect/src/{engine,claude}*`, `crates/lotse-hook/**`, `integrations/claude/**`
- **Scope:** engine state machine per session and sub-agent (SubagentStart/Stop keep the lead in progress); Claude adapter mapping hook events (incl. `PermissionRequest`, `Stop`, `StopFailure`, `SubagentStart`, `SubagentStop`, `PostToolUseFailure`, `UserPromptSubmit`, `SessionStart`/`SessionEnd`) to status; `lotse-hook` reads hook JSON from stdin, adds session id from `LOTSE_SESSION_ID` env, sends to the socket with the token, exits 0 fast and never blocks the agent (Alpha does not answer permission decisions); installer/uninstaller that merges into Claude settings idempotently with a backup and a `lotse` marker.
- **Rules:** verify event names and payload fields against the current Claude Code hooks docs and record the doc URL in the code; never invent fields. **Tests use a temp HOME / `CLAUDE_CONFIG_DIR` and recorded fixtures; never touch `~/.claude`, never run a real agent in tests.**
- **Done when:** fixture-driven tests cover every transition in the status table; installer round-trip (install → uninstall) leaves settings byte-identical to the backup.
- **Mode:** harnex.

### WP3 · App shell, sidebar, status bar (UI, against MockClient)
- **Owns:** `app/src/{sidebar,statusbar,lib/state}/**`, sidebar parts of `app/src/shell/**`
- **Scope:** everything in `spec/research.html#sidebar` and the Alpha status bar: tree, project row (tile, chevron on hover, collapsed roll-up counts), session cards (title, agent · branch line, coarse time, unread dot, sub-agent fold, Allow/Always/Deny always visible on `perm` cards — buttons call the client but Alpha only focuses the terminal, Retry swap on `fail`), resize 200–420 with persistence, hide ⌃⌘S, scroll, truncation, ↑/↓ navigation, need-you filter, project and session context menus, Dock-badge count source, status bar (need-you, git, daemon state, session count, bell, LAN Off, tokens `—`). No per-second re-render (rule: no layout shift, stable under pointer).
- **Done when:** Vitest component tests for roll-up, filter, keyboard nav, truncation; Playwright run against `npm run dev` (MockClient) checks resize, hide, 30-project scroll, light/dark screenshots matching the mock's look.
- **Mode:** harnex (includes its UI/UX review).

### WP4 · Terminal panes
- **Owns:** `app/src/{terminal,panes}/**`, tabs slot in `app/src/shell/**`
- **Scope:** xterm.js with WebGL (canvas fallback), fit, attach/replay through `DaemonClient`, input, resize, links; tabs per project; splits up to 4 (⌘D, ⌥-click, drag card onto terminal, menu), resizable dividers, ⌘[ ⌘] focus, ⌘W close; terminal font family/size settings and ⌘+ / ⌘−; iTerm2-like look from the mock.
- **Done when:** MockClient echo session works in the browser; tests for pane add/close/focus/resize; Playwright checks typing round-trip and split/resize.
- **Mode:** harnex.

### WP5 · Sheets, Settings, palette
- **Owns:** `app/src/{sheets,settings,palette}/**`
- **Scope:** Add project (Browse via Tauri dialog, Clone URL, Create new with optional git init, drag folder; multi-repo folder offers each repo); New session sheet (project, Terminal default, agents, optional worktree with branch field, first prompt for agents, shows the command); project colour and icon picker (15 swatches, searchable symbol grid, persisted); Settings full page (Agents launch commands with Reset and detected version, Notifications toggles, Appearance theme and both fonts, Remote shown Off, Privacy statement, Shortcuts list); ⌘K palette; ⌘/ shortcuts; ⌘T, ⇧⌘O, ⌘, bindings.
- **Done when:** component tests for each form's validation; Playwright walks each sheet against MockClient in light and dark.
- **Mode:** harnex.

### WP6 · Codex and OpenCode detection
- **Owns:** `crates/lotse-detect/src/{osc7501,screen,opencode}*`, `integrations/{opencode,manifests}/**`
- **Scope:** OSC 7501 parser fed from the PTY stream; screen-scrape adapter over the VT model with TOML manifests (bundled, user override dir, no fetch); Codex manifest; OpenCode plugin that reports status to `lotse-hook`; process-group identity to tell which agent runs in a pane.
- **Rules:** verify OSC 7501 and OpenCode plugin APIs against Orca's source in `/Users/naim/Repos/research/orca` and OpenCode docs; label anything unverified.
- **Done when:** recorded PTY transcripts (fixtures) replay to the expected status sequence for Codex and OpenCode.
- **Mode:** harnex quick.

### WP7 · Git, notifications, native helpers
- **Owns:** `crates/lotse-git/**`, `native/**`, `app/src-tauri/src/notify*`
- **Scope:** git info via the `git` CLI (branch, ahead, dirty count, worktree detection, "no git"); create worktree for a new session under `~/.lotse/worktrees/<project>/<branch>`; macOS notifications per event (perm, fail, done) honouring global toggles and the pause bell (⌥⌘N); Dock badge = need-you count; optional Swift SF Symbols helper (`NSImage(systemSymbolName:)` → PNG cache).
- **Done when:** lotse-git tests on temp repos (ahead, dirty, worktree add); notification decision logic unit-tested; manual check of one real notification recorded in the report.
- **Risk:** notifications from unsigned dev builds may show under the wrong app identity (unverified). Alpha accepts it.
- **Mode:** harnex quick.

### WP8 · Integration (serial)
- **Owns:** `app/src-tauri/**`, `app/src/lib/client/real*`, wiring in `crates/lotsed/src/{detect_glue,git_glue}*`
- **Scope:** Tauri bridge to the real daemon (commands, events, PTY channel); detection and git wired into the daemon; app starts the daemon if needed; real client replaces MockClient behind a flag (MockClient stays for UI dev); end-to-end flows.
- **Done when:** scripted e2e: start daemon → add a temp project → open a shell session → status events flow to the UI store → quit and relaunch the app → session still there with scrollback. Manual: one real Claude Code session goes run → perm → run → done in the sidebar.
- **Note:** `tauri-driver` WebDriver does not support macOS, so app-level e2e is Rust integration tests plus Playwright on the web build.
- **Mode:** harnex.

### WP9 · Hardening and Alpha build (serial)
- **Scope:** security baseline checks (no TCP listeners, file modes, token required, hooks uninstallable, CSP review, capability review, `cargo audit`/`npm audit` clean or justified); local `.app` build (ad-hoc signed); README install and uninstall; dogfood checklist from the Alpha exit criteria; GitHub issue templates for feedback.
- **Done when:** checklist green; Naim installs the build.
- **Mode:** harnex quick.

## Order and parallelism

```
WP0 ──► wave 1: WP1 ∥ WP2 ∥ WP3
        wave 2: WP4 ∥ WP5 ∥ WP6, then WP7 (when a slot frees)
        ──► WP8 (serial) ──► WP9 (serial)
```

- At most **3 runs at once** (Rust builds and Playwright are heavy). A wave-2 WP may start as soon as any slot frees; its only hard dependency is WP0.
- Merge order inside a wave: daemon/crates first, UI after, so UI merges conflict only with UI.
- Owns-paths are disjoint by design. Shared files (`Cargo.toml`, `package.json`, lockfiles, `app/src/shell/*`) belong to the orchestrator after WP0; a WP that needs a change there asks in its report.

## Orchestrator runbook

**Prerequisites (orchestrator, before WP0):** `cargo tauri` is not needed globally (the npm `@tauri-apps/cli` is used). Install `sccache` (`brew install sccache`) and export `RUSTC_WRAPPER=sccache` in `~/.zshrc.local` so delegated runs inherit it (ask Naim first: it is his file). Confirm `gh auth status` (account `tankibaj`).

**Per WP:**
1. `git -C lotse worktree add ../lotse-wt/<slug> -b feature/<slug> main`
2. In the worktree: `npm ci` in `app/`; per-run `CARGO_TARGET_DIR=<worktree>/target`; `VITE_PORT=1420+N` (N = slot 1–3) in the brief.
3. Write the brief: WP section from this plan verbatim, owns-paths, the mock parity gate (step 5) as part of done-when, the harnex headless contract (no questions, defaults apply and open questions go in the report; the WP section is the approved plan; stay on the current branch, no worktrees, no commits; run every subagent in the foreground and never end the turn while one runs), "the spec is `spec/` and this plan is approved and read-only", headless rules, return format. Launch: `DELEGATE_PROTECT=ALPHA_PLAN.md,spec/mock.html,spec/research.html DELEGATE_BUDGET_USD=15 DELEGATE_TIMEOUT_MIN=90 D run claude rw <worktree> <rundir>` in the background.
4. On notify: `D result`; `git diff --stat`; check owns-paths were respected; run the WP's done-when commands yourself; spot-read risky hunks (socket, file modes, installer, CSP).
5. **Mock parity gate** (every WP, before merge). Compare the WP's features with the same features in `spec/mock.html`:
   - **UI WPs (WP0 shell, WP3, WP4, WP5, WP8):** open the mock and the app (`npm run dev`, MockClient seeded with the mock's data) side by side at the same viewport (1440×900 and 1280×800), light and dark. Screenshot each related view and state (e.g. WP3: expanded/collapsed projects, perm and fail cards, need-you filter, 200 px and 420 px widths). Walk the related interactions in both (hover, keyboard, menus, shortcuts). Check against the rules in `spec/research.html#sidebar` / `#panels`.
   - **Non-UI WPs (WP1, WP2, WP6, WP7, WP9):** compare behaviour with what the mock shows: status names and roll-up order, sub-agent states, git line fields (branch, ↑ahead, ±dirty, worktree, "No git"), launch-command defaults, notification events and toggles. Proto fixtures must be able to reproduce every state the mock displays.
   - Record a parity table in `STATUS.md` (feature · mock · app · match / gap). Gaps either get fixed in a resumed run or are logged as accepted deviations with a reason. Screenshots go in `parity/<wp>/` (committed, small PNGs).
   - Delegated runs do this themselves first (part of the brief and of harnex's UI/UX review); the orchestrator re-checks the screenshots and the table, not just the claim.
6. Commit in the worktree (`feat(<scope>): …`, no trailers), merge into `main` (`--no-ff`), run the full CI command set on `main`, push `main`.
7. Roll `status/WPn.md` into `STATUS.md`, update metrics, commit `status/`. Then remove the worktree and delete the merged branch (pre-authorized by Naim only after merge and verification).
8. Update `STATUS.md`: state, branch, run dir, session time, cost, bounce-backs, verification line, parity result, merge commit.

**Progress reporting by delegated agents (in every brief):**
- Each agent keeps `~/Repos/incubator/lotse/status/WPn.md` (absolute path, main checkout) from `status/_TEMPLATE.md`. **One writer per file**; agents never edit `STATUS.md` (parallel runs would overwrite each other).
- Write it at start (tasks + first ETA), at every phase change (plan → implement → test → parity → review → done), after every test-suite run, on any handoff or bounce-back, when blocked, at least every ~20 minutes of work, and at the end (final report). Revise the ETA with a reason whenever it moves.
- Timestamps come from `date '+%F %R'`; launch runs with `DELEGATE_ALLOW='Bash(date:*)'`.
- Assumption to confirm on WP0: the delegated Claude CLI can Edit a path outside its worktree. If not, the agent writes `status/WPn.md` inside its worktree and the orchestrator copies it to the main checkout at each wake-up.
- The orchestrator rolls each file up into `STATUS.md` (table + metrics) on every wake-up and at merge, and commits `status/` with the merge.

**Delivery metrics (orchestrator keeps them in `STATUS.md`):**
- **Session time:** per run, wall-clock from launch to result (delegate `started` file → `result`), plus time blocked or waiting on review; totals per WP and for Alpha.
- **Handoffs and bounce-backs:** counts per WP. Handoff = work passed on (agent → orchestrator request, WP → WP, escalation to Naim). Bounce-back = work returned (harnex review CHANGES, orchestrator rejection, failed done-when, parity gap, resumed follow-up run).
- **Change failure rate:** merges into `main` that needed a fix or revert (CI red on `main`, broken build, regression found by a later WP) ÷ all merges. Target ≤ 15 %.
- **Escaped bugs:** defects found after a WP merged (by a later WP, WP8 integration, WP9 hardening or Naim's dogfood), attributed to the WP they came from, with severity and the fix commit.
- **Cost:** USD per run from `D result`.

**Failure handling:** a failed run gets one resumed follow-up with the exact failing output. A second failure stops that WP and goes to Naim with the evidence. Never merge red.

**Context hygiene:** never read agent logs; read `result`, diff stat and risky hunks only. After compaction, `STATUS.md` is the truth.

## Checkpoints with Naim

1. Approve this plan.
2. After WP0: demo of the empty shell next to the mock, and the frozen contracts.
3. After WP8: real Claude Code session in the app.
4. After WP9: install the Alpha build and start the 2-week dogfood.

## Open questions (defaults apply unless Naim changes them)

- Budget per run $15 and 90 min; total Alpha budget not set.
- `sccache` export in `~/.zshrc.local`: needs Naim's OK.
- Push `main` to GitHub after every verified merge: assumed yes.
