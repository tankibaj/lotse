# Codex

Codex has no injected hooks yet; its status comes from the screen manifest (`../manifests/codex.toml`).

Why not, the way Claude Code and OpenCode get a plugin per pane:

- Codex reads hooks from `hooks.json` in `CODEX_HOME` and only runs a hook whose hash is trusted in that
  home's `config.toml` (`[hooks.state."<hooks.json path>:<event>:<group>:<handler>"] trusted_hash =
  "sha256:..."`; the hash algorithm is in Orca's `src/main/codex/codex-trust-identity.ts`). There is no
  environment variable or flag that adds a hook for one process without one of those two files.
- Writing them into the user's `~/.codex` is a global edit, which is what this work avoids. The other way
  is Orca's mirrored `CODEX_HOME` per account (`src/main/codex/codex-config-mirror*.ts`,
  `codex-session-bridge.ts`): `config.toml` is copied and kept in sync both ways, and `auth.json` and the
  session store are bridged so a token refresh or a trust decision made in the pane is not lost. That is a
  feature of its own; a plain symlinked home would let a refreshed `auth.json` replace the link and
  strand the user's real credentials.
- `lotse-detect` has no Codex hook adapter yet. Codex does have `SubagentStart` / `SubagentStop`
  hook events (listed in Orca's `CODEX_EVENTS`, and in the Codex 0.162.1 binary), so an adapter modelled on
  `claude.rs` would give sub-agents once the delivery above exists.

The daemon does drop the variables a parent Codex exports (`CODEX_THREAD_ID`, `CODEX_SANDBOX`, ...) from
pane environments; `CODEX_HOME` and `CODEX_API_KEY` are kept.
