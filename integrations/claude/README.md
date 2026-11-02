# Claude Code integration

Lotse reads Claude Code's status from hooks. `lotse-hook` is the command the hooks run; it forwards the
hook JSON (minus the bulky `tool_response`) to the `lotsed` Unix socket (token first) and always exits 0
without printing anything.

| File | What |
|---|---|
| `hooks.json` | The hooks Lotse installs: events, matchers, timeout, and the `# lotse:managed` marker. Embedded into `lotse-hook` at build time. |
| `fixtures/*.json` | Hook-payload scenarios with the status the engine must report after each step. Authored from the hooks reference, **not recorded** from a live agent. Run by `crates/lotse-detect/tests/claude_fixtures.rs`. |

Event names and payload fields: <https://code.claude.com/docs/en/hooks>. **Unverified against the live
page** (no web access when written); checked against Orca's and herdr's Claude integrations instead. The
status table is in `crates/lotse-detect/src/claude.rs`.

## Install, status, uninstall

```sh
lotse-hook integration claude install     # adds one command hook per event to ~/.claude/settings.json
lotse-hook integration claude status      # installed | partial | not installed
lotse-hook integration claude uninstall
# --config-dir DIR overrides $CLAUDE_CONFIG_DIR, which overrides ~/.claude
```

- Each command is `'<path to lotse-hook>' claude # lotse:managed`. Only commands containing the marker are
  ever changed or removed; your own hooks, even in the same group, stay.
- Installing twice changes nothing. A moved binary is updated in place.
- The first install copies the original bytes to `settings.json.lotse-backup` (mode 0600). Uninstall
  restores them byte for byte when nothing else changed in between; otherwise it removes only the Lotse
  hooks and keeps your edits.
- A `settings.json` that is not strict JSON (comments, trailing commas) is left alone and reported.
- Outside a Lotse session (`LOTSE_SESSION_ID` unset) the hook does nothing.

## Environment

| Variable | Set by | Meaning |
|---|---|---|
| `LOTSE_SESSION_ID` | `lotsed`, on every PTY it spawns | Session the event belongs to |
| `LOTSE_DATA_DIR` | optional | Directory with `lotsed.sock` and `token`; default `~/Library/Application Support/Lotse` |
| `LOTSE_HOOK_DEBUG` | you | Print why an event was not delivered (stderr) |
