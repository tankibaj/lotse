# Codex

Lotse reads Codex's status and sub-agents from hooks, per pane, with nothing written to your `~/.codex`.
Until the first hook arrives (and for anything the hooks do not report) the screen manifest
(`../manifests/codex.toml`) is the source; after that the hooks win.

| File | What |
|---|---|
| `fixtures/*.json` | Hook-payload scenarios with the status the engine must report after each step. Shaped like Codex 0.162.1's payloads, **not recorded** from a live agent. Run by `crates/lotse-detect/tests/codex_fixtures.rs`. |

The status table is in `crates/lotse-detect/src/codex.rs` (it reuses the Claude Code adapter,
`claude.rs`, with the differences listed there). The mirror is `crates/lotse-hook/src/codex_home.rs`.

## How it works

Codex only runs a hook that is in `hooks.json` of its `CODEX_HOME` *and* has a trusted hash in that home's
`config.toml` (`[hooks.state."<hooks.json path>:<event>:<group>:<handler>"] trusted_hash = "sha256:..."`).
There is no flag or variable that adds a hook for one process, so every pane `lotsed` starts gets
`CODEX_HOME=<data dir>/integrations/codex/<hash of your home>/`, a mirror of your Codex home (the way
OpenCode gets an overlay `OPENCODE_CONFIG_DIR`; Orca does the same with managed homes,
`src/main/codex/codex-config-mirror*.ts`):

- Every entry of your real `$CODEX_HOME` (default `~/.codex`; `LOTSE_CODEX_SOURCE_HOME` in the pane names
  it) is a symlink: sessions, history, logs, databases, `auth.json`, skills, `AGENTS.md`. Codex reads and
  writes your real files through them; relative paths in your `config.toml` should resolve to them too (not exercised).
- `hooks.json` is a real file: your hooks, then one Lotse handler per event after yours
  (`SessionStart`, `UserPromptSubmit`, `PreToolUse`, `PermissionRequest`, `PostToolUse`, `SubagentStart`,
  `SubagentStop`, `Stop`, `Interrupt`; names checked in the 0.162.1 binary), each running
  `"${LOTSE_HOOK_BIN:-lotse-hook}" codex --injected`. Your groups keep their indexes, so their trust stays valid.
- `config.toml` is a real file (mode 0600): your config, a trust entry per Lotse hook in Orca's format, and
  your own hooks' trust re-keyed to the mirror's paths. The hash is `sha256` of the canonical JSON
  `{"event_name", "hooks": [{"async": false, "command", "timeout", "type": "command"}]}`
  (Orca `src/main/codex/codex-trust-identity.ts`). It was compared with the `currentHash` Codex 0.162.1 returns
  from `hooks/list` for all nine events, and with that mirror Codex lists every hook (yours too) as `trusted`.
- Both files are rebuilt only when the size or mtime of your `config.toml` or `hooks.json` changes (and when
  Lotse's hook definition does). A config Lotse cannot add to without breaking it, or invalid JSON in
  `hooks.json`, leaves the pane on your real home with no hooks (a warning is logged).
- Codex >= 0.157 starts a daemon on `$CODEX_HOME/app-server-control/app-server-control.sock`. The mirror path
  under `~/Library/Application Support/Lotse/` is longer than a macOS socket path (104 bytes), so the mirror's
  `config.toml` sets `[features] daemon_auto_start = false` (as Orca does for long homes). Your own
  `daemon_auto_start` wins when set. Your real config is not changed.

`lotse-hook codex` forwards the payload like the Claude one: no `tool_response`, no prompt or reply text, only
the summary key of `tool_input`. The daemon drops the twin of an event when the same hook is also installed
globally (`--injected` tags the pane's copy).

### `auth.json` and other writes

Codex refreshes tokens in `auth.json`; the mirror must not strand them. Through the symlink an in-place write
lands in your real file. If Codex replaces the file instead (write a temp file, rename it over `auth.json`), the
link is replaced by a regular file in the mirror. Every time a pane is created Lotse looks for that: a regular
file in the mirror whose mtime is newer than your real file's is copied over it (temp file + rename,
permissions kept, a symlinked real file is followed), an older one is dropped, and the link comes back.
Entries Codex creates in the mirror that your home does not have yet (`sessions/`, a login when you had none, a
new database) are moved into your real home and linked. Tests: `auth_json_that_codex_replaced_by_rename_lands_in_the_real_file`,
`a_real_auth_json_that_changed_since_wins_over_a_stale_mirror_copy`, `a_login_made_in_the_mirror_is_kept_when_the_real_home_had_none`.
The sync runs at pane creation, not continuously (Orca polls): a token refreshed by a pane that replaced the file is
in your real `auth.json` after the next Lotse pane starts, not before. Run `codex` outside Lotse before that and it
uses the previous token.

## Turn it off

- Everywhere: `export LOTSE_CODEX_HOOKS=off` (`0`, `false`, `no` too) in your login shell's environment, then restart
  `lotsed` (quit Lotse). Panes get no `CODEX_HOME`; Codex runs on your real home and Lotse falls back to the screen.
- One command: `CODEX_HOME=$LOTSE_CODEX_SOURCE_HOME codex` in a pane.
- Clean up: delete `~/Library/Application Support/Lotse/integrations/codex/`. It holds symlinks and two generated
  files; the real files they point to are not removed by deleting the links. Check for a regular `auth.json` in a
  mirror first (a pane may not have synced it yet) and copy it over `~/.codex/auth.json` if it is newer.

No panes get the mirror while `~/.codex` (or your `CODEX_HOME`) does not exist: run `codex` once, then open a new pane.

## Limits and unverified

- **Not run against a live Codex turn.** The hook trust (`hooks/list` says `trusted`) and the hash were checked
  against the installed 0.162.1 in a scratch home; that Codex then runs `"${LOTSE_HOOK_BIN:-lotse-hook}" codex --injected`
  through a shell with the pane's environment and the JSON on stdin was not (Orca's hook relies on the same).
  Payloads in the fixtures are authored from the field names in the binary (`hook_event_name`, `session_id`,
  `turn_id`, `agent_id`, `agent_type`, `transcript_path`, `model`, `permission_mode`, `tool_name`, `tool_input`,
  `tool_use_id`, `source`) and Orca's handling, not recorded. Other Codex versions: hashes or event names may differ.
- **No `Fail` from hooks.** Codex has no failure hook. A turn that fails (network, API error) fires no hook, so the
  status stays `run` until the next prompt or `Interrupt`, and the screen cannot correct it while hooks win
  (`TODO(codex-failed-turn)` in `codex.rs`). Esc is reported (`Interrupt` -> `done`).
- With Codex's approval auto-review on, `PermissionRequest` also fires for requests its reviewer answers, so `perm`
  can flash (Orca reads the session rollout to tell them apart).
- Settings Codex writes to the mirror's `config.toml` (a project trusted in a pane, `/model`) stay in the mirror
  until your own `config.toml` changes, then are discarded; they never reach `~/.codex/config.toml`. Same for
  `hooks.json`.
- A Codex upgrade that creates a database in the mirror while another pane's Codex has it open is moved into your
  home by the next pane's sync; the open Codex keeps working on the moved file.
- Keychain/`secret_auth_storage` credential storage (no `auth.json`) is untouched by all this and not exercised here.
