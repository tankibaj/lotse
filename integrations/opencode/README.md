# OpenCode integration

`lotse-status.js` is an OpenCode plugin. It watches OpenCode's session events and tells Lotse when a
session is busy, idle, failed, or waiting on a permission or question, so the sidebar card follows.
It sends no conversation text and never fails OpenCode (every error is swallowed). Outside a Lotse pane
(`LOTSE_SESSION_ID` unset) it does nothing.

For each event it runs `lotse-hook opencode` with one JSON object on stdin, one at a time and in order;
`lotse-hook` forwards it to the `lotsed` Unix socket. The payload contract and the status table are in
`crates/lotse-detect/src/opencode.rs`.

| File | What |
|---|---|
| `lotse-status.js` | The plugin. |
| `fixtures/*.json` | OpenCode events, the payloads the plugin must send for them, and the status each payload leads to. Run by `crates/lotse-detect/tests/opencode_fixtures.rs`, which executes the real plugin under `node` with a fake `lotse-hook`. |

## Install and remove (manual, for now)

```sh
mkdir -p "${XDG_CONFIG_HOME:-$HOME/.config}/opencode/plugins"
cp integrations/opencode/lotse-status.js "${XDG_CONFIG_HOME:-$HOME/.config}/opencode/plugins/lotse-status.js"
# remove: rm "${XDG_CONFIG_HOME:-$HOME/.config}/opencode/plugins/lotse-status.js"
```

`lotse-hook` must be on `PATH`, or `LOTSE_HOOK_BIN` set to it (the daemon should set it on every pane it
spawns). A `lotse-hook integration opencode install|uninstall|status` command, like the Claude one, is
requested in the WP6 report; it belongs to `crates/lotse-hook`.

## Verified and unverified

Taken from Orca's OpenCode status plugin (`research/orca/src/main/opencode/status-plugin-*-source.ts`):
the plugin shape (`async (ctx) => ({ event })`, default export `{ id, server }`), the config location
(`$XDG_CONFIG_HOME/opencode/plugins/`, else `~/.config/opencode/plugins/`), the event names, and the
`sessionID`, `info.parentID`, `status.type` and request `id` fields.

**Unverified** (no web access when written; check <https://opencode.ai/docs/plugins/> and the event
reference): the `permission.asked` fields that feed the card's tool and summary (`permission`,
`patterns`; the older `type`, `pattern`, `title`), `question.asked`'s `questions[0].question`, the
`session.error` shape (`error.name`, `error.data.message`), and whether the plugin must be listed in
`opencode.json` instead of being picked up from the `plugins/` directory. A missing field only blanks
the card's summary; it never changes the status.

## Injected per pane (no install)

`lotsed` writes this plugin to `<data dir>/integrations/opencode/default/plugins/lotse-status.js` and sets
`OPENCODE_CONFIG_DIR` to that directory in every pane. OpenCode adds `$OPENCODE_CONFIG_DIR` to its config
directories (its global one stays), so nothing in `~/.config/opencode` changes. If the pane already has an
`OPENCODE_CONFIG_DIR`, the daemon builds `<data dir>/integrations/opencode/<hash>/` that symlinks every
entry of that directory (and of its `plugins/`) next to the plugin, and points the pane there.

With the plugin installed globally too, the copy that loads second in the same OpenCode process stays inert
for that project directory, so events are reported once. Child sessions (`session.created` with `parentID`)
arrive as `parent_id` and become sub-agents.
