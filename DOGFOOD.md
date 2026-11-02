# Dogfood checklist (Alpha)

Install per `README.md` (`sh scripts/build-app.sh`, copy to `/Applications`). File every finding with the
GitHub issue templates (`bug`, `feedback`): version (`Lotse.app` › About, or the commit), macOS version, agent and
agent version, steps. Never paste tokens, `.env` values or session transcripts.

## 1. Alpha exit criteria (`spec/research.html`, Release 1)

- [ ] **Daily driver for 2 weeks** across 3 or more projects and all 3 agents (Claude Code, Codex, OpenCode).
  Tally: days used `__ / 14` · projects `__` · Claude Code `[ ]` Codex `[ ]` OpenCode `[ ]`.
- [ ] **Status matches reality at least 95% of the time.** Keep a tally for a few sessions per agent: each time you
  look at a card and it is wrong (shows run while the agent waits, perm missing, done too early/late), note it.
  95% = at most 1 wrong in 20 glances. Record agent, version, status shown vs. real, and whether hooks/plugin were
  installed.

  | Agent | Glances | Wrong | % right |
  |---|---|---|---|
  | Claude Code | | | |
  | Codex | | | |
  | OpenCode | | | |
- [ ] **No session is lost when the app restarts.** Quit with ⌘Q and reopen with sessions running, waiting and done:
  every card is back with its status, and the terminal replays its scrollback. Also: kill the app (Activity Monitor)
  and reopen; `kill` the daemon and see the app restart it (sessions end then, by design in Alpha).
- [ ] **Feedback is logged as issues** (every item below that fails becomes one).

## 2. Manual checks the work packages could not run headless

Mark each `✓` or open an issue.

### Real agent runs (WP8)
- [ ] Settings › Agents › Claude Code › **Install hooks**. Check `~/.claude/settings.json` has `# lotse:managed` entries and `settings.json.lotse-backup` exists.
- [ ] Add a project, New session › Claude Code, give it a task that needs a tool permission. Expect the card:
  **run** (spinner) → **perm** (Allow/Always/Deny shown, Dock badge 1, banner "<project> · Claude Code / Needs permission") →
  after you answer in the terminal **run** → **done** (unread dot, banner "Finished"). Clears the badge.
- [ ] Deny a permission in the terminal: the card leaves **perm** (and does not stay stuck).
- [ ] A turn with a sub-agent (ask Claude Code to use a sub-agent): sub-agent rows appear and finish; a failing sub-agent shows **fail**.
- [ ] `claude --version` is at least the version the hooks need (`PermissionRequest` ≥ 2.0.45, `StopFailure` ≥ 2.1.78, from Orca's notes, **unverified** against the docs). On an older Claude Code, check that it does not reject `settings.json` for unknown hook names; if it does, Remove hooks and report it.
- [ ] **Remove hooks**: the `# lotse:managed` entries are gone, your own hooks are untouched, the file is as before.
- [ ] Hook payloads: event names and fields match the live docs (<https://code.claude.com/docs/en/hooks>); a card summary shows the real tool and command.

### Codex (WP6)
- [ ] `codex --version` of the installed version. Detection (screen manifest) was checked against recordings of 0.155.1, 0.157.x and 0.158.0 only. In a Codex session: working → **run**, approval prompt → **perm** (command shown), finished → **done**, update dialog / trust prompt do not read as perm.
- [ ] Esc-interrupting a Codex turn does not show **fail**; a real model-connect error does.

### OpenCode (WP6)
- [ ] Settings › Agents › OpenCode › **Install plugin** writes `~/.config/opencode/plugins/lotse-status.js` (or `$XDG_CONFIG_HOME`).
- [ ] OpenCode **loads** it from that folder (no entry in `opencode.json` needed). Verify: status follows a turn (run → done), a permission shows **perm**, the sub-agent session shows as a sub-agent. If OpenCode does not load it, note the OpenCode version (<https://opencode.ai/docs/plugins/>).
- [ ] `permission.asked` / `question.asked` fields fill the card's tool and summary (a missing field only blanks the summary).
- [ ] **Remove plugin** deletes only Lotse's file.

### Notifications (WP7, WP8)
- [ ] Banner appears for perm ("Needs permission") and done ("Finished"), with the macOS sound; none for sub-agent events (default) and none for shell sessions.
- [ ] Dock badge = number of sessions that need you; clears when none.
- [ ] Pause notifications (⌥⌘N) silences banners and sound; the bell shows the paused state.
- [ ] Notification identity: macOS may list the app as "Lotse" (signed ad hoc) or as the launching terminal in dev builds; check System Settings › Notifications.

### Terminal and window (WP4, WP3, WP5)
- [ ] **WebGL on Retina:** text sharp, no mis-scaled glyphs, resize and split OK. If not: set `lotse-term-renderer=dom` in the webview's local storage and report.
- [ ] Links in the terminal (cmd-click an `https://…` URL) open in Safari; `file:`/`javascript:` links do nothing.
- [ ] ⌘ shortcuts reach the app, not the shell: ⌘K, ⌘J, ⌘T, ⌘1–9, ⌘/, ⌘, , ⇧⌘O, ⌥⌘N. Ctrl+Tab. Click-to-focus. Non-QWERTY layout: ⌘J and ⌥⌘N still work.
- [ ] Title bar: drag moves the window, double-click zooms it (the capability set was reduced to the permissions the UI uses).
- [ ] Drag a folder onto the window → Add project sheet with the path. Folder picker (native dialog) works.
- [ ] Add project: scan a folder of repos, clone a URL into a new folder, create a new folder with `git init`.
- [ ] Reveal in Finder, Open in Split Right (⌥-click), Stop Session, Retry on a failed session (refused with a message while the process is alive).
- [ ] Worktree session: creates `~/.lotse/worktrees/<project>/<branch>`; card shows branch, ↑ahead, ±dirty, worktree marker; a non-git folder shows "No git".
- [ ] First prompt typed into a new session (launch option) reaches Claude Code / Codex / OpenCode intact.
- [ ] Login environment: sessions see the PATH from your `.zshrc` (nvm, brew). A very slow `.zshrc` (> 10 s) falls back to the daemon's environment until restart.
- [ ] Many sessions (10+): UI stays responsive; a 200 MB `cat` flood does not freeze the app.
- [ ] Symbols helper (optional, WP7): `sh native/symbols/build.sh` then `native/symbols/build/lotse-symbols folder 16 regular /tmp/x` renders a PNG. Never compiled in the WP run; Lucide is the fallback.

### Security spot checks (WP9)
- [ ] `lsof -nP -iTCP -a -p $(cat "$HOME/Library/Application Support/Lotse/lotsed.pid")` and the same for the app's pid print nothing.
- [ ] `ls -ld "$HOME/Library/Application Support/Lotse"` is `drwx------`; `lotsed.sock`, `token`, `lotse.db`, `lotsed.pid`, `lotsed.log` are `-rw-------` / `srw-------`.

## 3. Decisions still open for Naim

Not changed by WP9; each needs an answer (or "keep").

1. **`PostToolUseFailure` → run or fail (WP2).** Today it maps to **run**: the error goes back to the model and only `StopFailure` is **fail**. `research.html` lists it among the hooks that give "failed"; a red card on a failed tool is one arm in `handle_hook` (`crates/lotse-detect/src/claude.rs`). Keep **run**, or switch to **fail**?
2. **Light-theme contrast of the mock's tokens (WP3/WP5).** The UI reviewer measured orange text at 2.0:1, white on `#007AFF` at 4.0:1, and the blue links below AA on the light theme. The app matches the mock; a fix is a token change (`app/src/theme`). Keep the mock's tokens, or darken them for AA?
3. **`Event::Notification` banner flag (WP7/WP8).** "Banner off, sound on" currently sends nothing, because the proto event has no banner flag. Supporting it needs a `banner: bool` (or delivery kind) and a `PROTO_VERSION` bump. Do it before Beta, or drop the sound-only option?
4. **`tauri-plugin-opener` vs `/usr/bin/open` (WP8).** `ALPHA_PLAN.md` names the plugin; WP8 chose `open` with validated arguments (http/https only; `open -R` for Finder), which avoids a new dependency and a capability. Keep `open`?

## 4. Known limits (Alpha, by design)

Sessions do not survive a daemon restart or a reboot. Tokens and cost show `—`. Right panels, editor pane, Allow/Deny
from a notification and LAN access are Beta. Ad-hoc signed, not notarized. Apple Silicon only.
