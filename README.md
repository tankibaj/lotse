# Lotse

A macOS terminal app with projects and agent awareness. The main view is the full-size agent CLI (Claude Code, Codex, OpenCode or a shell); the sidebar shows every session's status (run, perm, fail, done, idle) and sub-agents.

Status: **Alpha**, Apple Silicon, macOS. Plan: `ALPHA_PLAN.md`, progress: `STATUS.md`, what to check while using it: [`DOGFOOD.md`](DOGFOOD.md). No telemetry, no network listeners (see [Privacy](#privacy)).

## Architecture
- `lotsed` (Rust daemon) owns the PTYs and state and listens on a Unix socket (`~/Library/Application Support/Lotse/lotsed.sock`).
- The app (Tauri 2, React, Tailwind, xterm.js) is a client; quitting it does not kill sessions.
- `lotse-hook` is the tiny CLI the agents' hooks call; it forwards status events to the socket.
- `lotse-proto` defines the wire protocol (`PROTO_VERSION = 1`); JSON fixtures in `crates/lotse-proto/fixtures` are the shared contract with the TypeScript `DaemonClient`.

## Install

Needs Rust 1.95 (`rust-toolchain.toml`), Node 22 and the Xcode command line tools (`codesign`).

```sh
git clone git@github.com:tankibaj/lotse.git && cd lotse
(cd app && npm ci)
sh scripts/build-app.sh          # release build, ad-hoc signature, smoke test (~5 min cold)
ditto target/release/bundle/macos/Lotse.app /Applications/Lotse.app
open /Applications/Lotse.app
```

`scripts/build-app.sh` puts `lotsed` and `lotse-hook` next to the app binary in `Lotse.app/Contents/MacOS` (the app finds them there), signs the bundle ad hoc (`codesign --force --deep -s -`), checks the signature and starts the bundled app against a temporary data folder to see the daemon socket appear. `--no-smoke` skips that last step.

The build is signed ad hoc and not notarized. Built on your own Mac it opens normally. A copy that came through a download or AirDrop is quarantined: `xattr -dr com.apple.quarantine /Applications/Lotse.app`, then open it (or right-click › Open).

## First run
1. Start Lotse. It starts `lotsed` in the background if its socket is missing, and creates `~/Library/Application Support/Lotse` (folder `0700`; socket, token and database `0600`).
2. **Add a project** (⇧⌘O): pick a folder, scan a folder for repositories, clone a URL, or create a new folder.
3. **New session** (⌘T): choose Claude Code, Codex, OpenCode or a shell. Launch commands are detected from your login shell's `PATH` and editable in Settings.
4. For exact status from Claude Code and OpenCode install their integrations (below). Codex gets its status and sub-agents per pane through a mirrored `CODEX_HOME` under the Lotse data folder (nothing in `~/.codex` changes; details and how to turn it off in `integrations/codex/README.md`), shells are detected from the terminal's own signals.
5. ⌘K jumps to any session or command, ⌘J to the next session that needs you, ⌘/ lists the shortcuts.

### Agent integrations (hooks)
Settings › Agents › Install hooks, per agent. Nothing is installed until you press the button.

| Agent | Install writes | Marker | Remove |
|---|---|---|---|
| Claude Code | hook entries in `~/.claude/settings.json` (or `$CLAUDE_CONFIG_DIR/settings.json`); the first change keeps `settings.json.lotse-backup` (`0600`) | every Lotse command ends in `# lotse:managed`; your own hooks are never touched | Settings › Agents › Remove hooks |
| OpenCode | `~/.config/opencode/plugins/lotse-status.js` (or under `$XDG_CONFIG_HOME`); never overwrites a file that is not Lotse's | header marker | Settings › Agents › Remove plugin |

The same from a shell, for example before deleting the app:

```sh
H=/Applications/Lotse.app/Contents/MacOS/lotse-hook
"$H" integration claude status      # installed | not installed | partial… | outdated…
"$H" integration claude uninstall
"$H" integration opencode uninstall
```

The hook commands point at the `lotse-hook` inside the app. After moving or rebuilding the app, press Install again (it updates the path in place). Outside a Lotse pane the hooks do nothing and exit 0, so an agent never fails because of them.

## Uninstall
Quit the app first (⌘Q): it restarts a daemon that died. Then:

```sh
H=/Applications/Lotse.app/Contents/MacOS/lotse-hook
"$H" integration claude uninstall                    # 1. remove hooks while the binary still exists
"$H" integration opencode uninstall
D="$HOME/Library/Application Support/Lotse"
kill "$(cat "$D/lotsed.pid")" 2>/dev/null            # 2. stop the daemon (this ends its sessions)
rm -rf /Applications/Lotse.app                       # 3. the app
rm -rf "$D"                                          # 4. database, token, socket, log
rm -rf "$HOME/Library/WebKit/dev.tankibaj.lotse" "$HOME/Library/Caches/dev.tankibaj.lotse" \
       "$HOME/Library/Saved Application State/dev.tankibaj.lotse.savedState"   # 5. webview data, if present
rm -f "$HOME/.claude/settings.json.lotse-backup"     # 6. optional: the settings backup
```

Worktrees Lotse created live in `~/.lotse/worktrees/<project>/<branch>`. Each is a normal git worktree of one of your repositories and may hold unmerged or uncommitted work, so they are not in the block above. Per repository: `git worktree list`, then `git worktree remove <path>` (and `git branch -d lotse/<branch>`). Once none is left: `rm -rf ~/.lotse`.

## Privacy
- **No telemetry**, no update checks, no remote manifests or assets at runtime. The only network traffic is what the programs you run in a session do themselves (and `git clone` when you ask for a clone).
- **No network listeners**: the daemon and the app listen on one Unix socket, `lotsed.sock` (`0600`, in a `0700` folder, peer uid checked). Every connection must send the token from `~/Library/Application Support/Lotse/token` (`0600`) first; a wrong or missing token is refused and closed. There is no TCP or UDP socket in the daemon or the app (the Vite dev server on `127.0.0.1:1420` exists only in `npm run dev`).
- The webview runs with a strict CSP (no remote origins, no `unsafe-eval`) and a minimal capability set.
- Hooks talk to the local socket only. The Claude Code hook sends the event, tool name, ids and the tool's command or path (the card summary), and leaves out prompts, replies, file contents and tool output; the OpenCode plugin sends session title and the permission or question text.- Tokens and cost in the status bar show `—` in Alpha.

## Develop
Requires Rust 1.95 (`rust-toolchain.toml`) and Node 22.

```sh
cargo fmt --all --check && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace
cd app && npm ci
cd app && npm run dev          # UI with the MockClient: http://127.0.0.1:1420 (VITE_PORT or `-- --port N` to change)
cd app && npm run tauri dev    # Tauri window
cd app && npm run lint && npm run typecheck && npx vitest run && npm run build
node scripts/licenses.mjs      # licence summary of what ships (see THIRD_PARTY_NOTICES.md)
```

Agents working on the repo: read `CLAUDE.md`. Feedback: GitHub issues (bug and feedback templates).
