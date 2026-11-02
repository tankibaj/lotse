# Screen manifests (Codex, OpenCode)

When an agent has no hook, Lotse reads its screen. `lotsed` keeps a VT model of every pane; once output
goes quiet it hands the visible rows to the screen adapter (`crates/lotse-detect/src/screen.rs`), which
asks a TOML manifest what they mean: working, waiting for you, or at rest. Manifests are data, compiled
into the daemon; **nothing is ever fetched**.

| File | What |
|---|---|
| `claude.toml` | Claude Code, as the fallback when no hook arrives: the spinner row (`esc to interrupt`), the `Do you want to proceed?` menu, background agents, the composer. Hook events always win. |
| `codex.toml` | Codex CLI. Working row (`• Working (11s • esc to interrupt)`), footer spinner, command approval and other dialogs, trust and update screens, transcript viewer. |
| `opencode.toml` | OpenCode, as the fallback when the Lotse plugin (`../opencode`) is not installed. |
| `fixtures/*.json` | Authored PTY sessions with the status the engine must report after each step. |
| `fixtures/recorded/*.json` | Real Codex PTY recordings (Orca project, MIT) with the status sequence they must replay to. |

Run by `crates/lotse-detect/tests/transcript_fixtures.rs`.

## Your own manifest

Drop `*.toml` files into `~/Library/Application Support/Lotse/manifests/` (or `$LOTSE_DATA_DIR/manifests/`).
A file whose `id` matches a bundled manifest replaces it; any other `id` adds one for the `agent` it names.
A file that does not parse is skipped and the reason is logged (`tracing` target `lotse_detect::screen`);
the bundled manifest stays in use. The format is documented at the top of
`crates/lotse-detect/src/screen/manifest.rs`; start from `codex.toml`.

## Provenance and limits

- `codex.toml` follows herdr's manifest (`research/herdr`, Apache-2.0) without its OSC-title rules, and
  was checked against Orca's recorded Codex 0.155.1 / 0.157.1 / 0.158.0 transcripts. The same engine
  replays all of them. **Unverified:** other Codex versions. When a Codex release changes its screen,
  edit a copy of this file (or record a new transcript and extend `fixtures/`).
- `claude.toml` has **no recorded transcript** either: it follows herdr's claude manifest, reduced to the visible rows, and is covered by authored fixtures (`claude_*.json`) only.
- `opencode.toml` has **no recorded transcript**: its phrases come from herdr and Orca. The OpenCode plugin
  is the primary path; treat this manifest as a best-effort fallback until a real OpenCode screen is recorded.
- Recorded fixtures: `cargo test -p lotse-detect --test transcript_fixtures export_orca_recordings -- --ignored`
  rewrites `fixtures/recorded/` from an Orca checkout (`$LOTSE_ORCA_FIXTURES`, default
  `/Users/naim/Repos/research/orca/src/main/runtime/__fixtures__`). The fixtures hold terminal bytes only.
