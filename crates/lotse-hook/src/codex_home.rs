//! The mirrored `CODEX_HOME` that gives a Codex pane Lotse's status hooks without touching the
//! user's `~/.codex`.
//!
//! Codex runs a hook only when `hooks.json` lists it *and* the `config.toml` next to it holds a
//! trusted hash for it (`[hooks.state."<hooks.json>:<event>:<group>:<handler>"]`); there is no flag
//! or variable that adds one hook for one process. So each pane gets `CODEX_HOME=<mirror>`, a
//! directory under the Lotse data dir (one per source home, like the OpenCode overlay) in which
//!
//! * every entry of the real home is a symlink (sessions, history, logs, databases, `auth.json`,
//!   skills, ...): Codex reads and writes the real files through it;
//! * `hooks.json` is the user's hooks plus Lotse's, one handler per event of [`EVENTS`] appended
//!   after the user's groups (so the user's group indexes, and their trust, stay valid);
//! * `config.toml` is the user's config plus the trust entry for each Lotse hook, and the user's own
//!   hook trust re-keyed to the mirror's paths. Both are real files, regenerated when the user's
//!   `config.toml` or `hooks.json` changes (size or mtime), never otherwise.
//!
//! The trust hash is Codex's: `sha256` of the canonical JSON of the hook identity, as Orca computes
//! it (`src/main/codex/codex-trust-identity.ts`). It matches `currentHash` from Codex 0.162.1's
//! `hooks/list` for all nine events (see the golden test). Nothing is ever written to the source
//! home, except what Codex itself wrote into the mirror and [`adopt`] moves back.
//!
//! **Why `adopt`:** Codex may replace a file instead of writing through it (rename over the path
//! swaps the symlink for a file) and creates entries the real home did not have yet (`sessions/`
//! in a fresh home, a new database). Left in the mirror, a refreshed `auth.json` token would never
//! reach the real home. On every call a regular file or directory in the mirror is therefore
//! moved or copied into the real home and replaced by the link: a file whose mirror copy is newer
//! overwrites the real one, an older mirror copy is dropped. Orca keeps its managed homes in sync
//! with polling (`src/main/codex-accounts/runtime-home-service-auth-sync.ts`); this runs when a pane
//! is created instead, so a refresh made by a pane is real after the next pane starts (or by
//! `lotsed`'s next call), and not immediately.

use crate::opencode_plugin::{fnv1a, mirror as link_entries};
use anyhow::{Context, Result, bail};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::UNIX_EPOCH;

/// The shell command of every hook. `LOTSE_HOOK_BIN` is exported into the pane by the daemon.
pub const HOOK_COMMAND: &str = r#""${LOTSE_HOOK_BIN:-lotse-hook}" codex --injected"#;

/// `hooks.json` event names with the snake_case label Codex uses in trust keys and hashes
/// (`codex-rs/hooks/src/lib.rs::hook_event_key_label`, via Orca's `CODEX_EVENTS`). Names verified in
/// the Codex 0.162.1 binary. `SessionEnd`, `PreCompact` and `PostCompact` exist there too but the
/// adapter has no use for them.
pub const EVENTS: [(&str, &str); 9] = [
    ("SessionStart", "session_start"),
    ("UserPromptSubmit", "user_prompt_submit"),
    ("PreToolUse", "pre_tool_use"),
    ("PermissionRequest", "permission_request"),
    ("PostToolUse", "post_tool_use"),
    ("SubagentStart", "subagent_start"),
    ("SubagentStop", "subagent_stop"),
    ("Stop", "stop"),
    ("Interrupt", "interrupt"),
];

/// The Codex release whose `hooks/list` `currentHash` values the golden test pins. A newer Codex may
/// hash a hook differently (it then asks to review Lotse's hooks): re-verify the table in the tests
/// against its `hooks/list` before trusting the scheme for it.
pub const VERIFIED_CODEX_VERSION: &str = "0.162.1";

/// Codex hashes the normalized timeout: at least 1, and `Interrupt` is clamped to 3.
fn timeout_seconds(label: &str) -> u64 {
    if label == "interrupt" { 3 } else { 10 }
}

const HOOKS_FILE: &str = "hooks.json";
const CONFIG_FILE: &str = "config.toml";
/// Records what the generated files were made from.
const STAMP_FILE: &str = ".lotse-sync";
/// Bump when the generated files change shape, so existing mirrors are rebuilt.
const LAYOUT: u32 = 1;
/// Files of the mirror that are generated, never linked.
fn generated(name: &str) -> bool {
    matches!(name, HOOKS_FILE | CONFIG_FILE | STAMP_FILE)
}

/// Longest Unix socket path: `sun_path` is 104 bytes on macOS (NUL included), 108 on Linux.
const SOCKET_PATH_LIMIT: usize = if cfg!(target_os = "macos") { 104 } else { 108 };
/// Codex >= 0.157 starts a daemon on `<CODEX_HOME>/app-server-control/app-server-control.sock`.
const DAEMON_SOCKET: &str = "app-server-control/app-server-control.sock";
const DAEMON_OFF_COMMENT: &str = "# lotse: CODEX_HOME is too long for the Codex daemon socket";

/// One mirror at a time: panes are created concurrently by the daemon.
static LOCK: Mutex<()> = Mutex::new(());

/// The trust hash Codex stores for a command hook: `sha256:` + hex of the canonical (sorted-key)
/// JSON of `{event_name, hooks: [handler]}`. A group matcher would be part of it; Lotse sets none.
pub fn trusted_hash(label: &str, command: &str, timeout: u64) -> String {
    let json = |s: &str| Value::String(s.to_owned()).to_string();
    let canonical = format!(
        r#"{{"event_name":{},"hooks":[{{"async":false,"command":{},"timeout":{timeout},"type":"command"}}]}}"#,
        json(label),
        json(command),
    );
    format!("sha256:{:x}", Sha256::digest(canonical))
}

/// The mirror of `source` (the user's `CODEX_HOME`) under `root`, created or brought up to date.
/// Returns the directory to export as `CODEX_HOME`. Any error means: leave the pane's `CODEX_HOME`
/// alone (Codex then runs against the real home, without Lotse hooks).
pub fn mirror(root: &Path, source: &Path) -> Result<PathBuf> {
    let _one_at_a_time = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let source = std::path::absolute(source).context("resolving CODEX_HOME")?;
    if !source.is_dir() {
        bail!("{} is not a directory", source.display());
    }
    let dir = root.join(format!(
        "{:016x}",
        fnv1a(source.as_os_str().as_encoded_bytes())
    ));
    std::fs::create_dir_all(&dir).with_context(|| format!("creating {}", dir.display()))?;
    adopt(&dir, &source)?;
    link_entries(&source, &dir, generated)?;
    refresh(&dir, &source)?;
    Ok(dir)
}

/// Move what Codex wrote into the mirror (instead of through a link) into the real home, and put
/// the link back. See the module docs.
fn adopt(mirror: &Path, source: &Path) -> Result<()> {
    for entry in std::fs::read_dir(mirror)?.filter_map(Result::ok) {
        let name = entry.file_name();
        if generated(&name.to_string_lossy()) {
            continue;
        }
        let local = entry.path();
        let local_meta = std::fs::symlink_metadata(&local)?;
        if local_meta.is_symlink() {
            continue;
        }
        let real = source.join(&name);
        match std::fs::metadata(&real) {
            // not in the real home (yet): it is Codex's data, it belongs there
            Err(e) if e.kind() == std::io::ErrorKind::NotFound && !is_link(&real) => {
                move_entry(&local, &real).with_context(|| format!("moving {}", local.display()))?;
            }
            Ok(real_meta) if local_meta.is_file() && real_meta.is_file() => {
                // the real file may itself be a link (dotfiles): replace what it points at
                let target = std::fs::canonicalize(&real)?;
                if local_meta.modified()? > real_meta.modified()? {
                    replace_file(&local, &target)
                        .with_context(|| format!("saving {}", target.display()))?;
                } else {
                    std::fs::remove_file(&local)?;
                }
            }
            // a directory both sides have, a file against a directory, a dangling link: leave it
            _ => {}
        }
    }
    Ok(())
}

fn is_link(path: &Path) -> bool {
    std::fs::symlink_metadata(path).is_ok_and(|m| m.is_symlink())
}

/// Rename into the real home; across volumes a file is copied (a directory stays in the mirror).
fn move_entry(from: &Path, to: &Path) -> Result<()> {
    match std::fs::rename(from, to) {
        Ok(()) => Ok(()),
        Err(_) if from.is_file() => {
            replace_file(from, to)?;
            Ok(())
        }
        Err(e) => Err(e.into()),
    }
}

/// Copy `from` over `to` through a temporary file, so a reader never sees half a file, and delete
/// `from`. Permissions are copied (`auth.json` is 0600).
fn replace_file(from: &Path, to: &Path) -> std::io::Result<()> {
    let tmp = temp_sibling(to);
    std::fs::copy(from, &tmp)?;
    std::fs::rename(&tmp, to).inspect_err(|_| {
        let _ = std::fs::remove_file(&tmp);
    })?;
    std::fs::remove_file(from)
}

fn temp_sibling(path: &Path) -> PathBuf {
    let mut name = OsString::from(".");
    name.push(path.file_name().unwrap_or_default());
    name.push(format!(".lotse-tmp-{}", std::process::id()));
    path.with_file_name(name)
}

/// What `hooks.json` and `config.toml` of the mirror were made from: size and mtime of the user's
/// two files and the layout. Same stamp, files present: nothing to do.
fn stamp(source: &Path) -> String {
    let of = |name: &str| match std::fs::metadata(source.join(name)) {
        Ok(m) => format!(
            "{}@{}",
            m.len(),
            m.modified()
                .ok()
                .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
                .map_or(0, |d| d.as_nanos())
        ),
        Err(_) => "-".to_owned(),
    };
    format!(
        "{LAYOUT} {:016x} {} {}",
        fnv1a(HOOK_COMMAND.as_bytes()),
        of(HOOKS_FILE),
        of(CONFIG_FILE)
    )
}

fn refresh(dir: &Path, source: &Path) -> Result<()> {
    let wanted = stamp(source);
    let have = std::fs::read_to_string(dir.join(STAMP_FILE)).ok();
    if have.as_deref() == Some(&wanted)
        && dir.join(HOOKS_FILE).is_file()
        && dir.join(CONFIG_FILE).is_file()
    {
        return Ok(());
    }
    let user_hooks = read_optional(&source.join(HOOKS_FILE))?;
    let user_config = read_optional(&source.join(CONFIG_FILE))?;
    let (hooks, groups) = build_hooks(user_hooks.as_deref())?;
    // Codex keys trust by the resolved home with the logical `hooks.json` leaf.
    let canonical = std::fs::canonicalize(dir)?;
    let config = build_config(
        user_config.as_deref().unwrap_or(""),
        &Paths {
            source: source.to_owned(),
            canonical_source: std::fs::canonicalize(source)?,
            mirror: canonical,
        },
        &groups,
    )?;
    write_private(&dir.join(HOOKS_FILE), &hooks)?;
    write_private(&dir.join(CONFIG_FILE), &config)?;
    // last: an interrupted refresh is redone
    write_private(&dir.join(STAMP_FILE), &wanted)
}

fn read_optional(path: &Path) -> Result<Option<String>> {
    match std::fs::read_to_string(path) {
        Ok(text) => Ok(Some(
            text.strip_prefix('\u{feff}').unwrap_or(&text).to_owned(),
        )),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e).with_context(|| format!("reading {}", path.display())),
    }
}

/// Owner-only (the config may hold MCP credentials), through a temporary file and a rename: the
/// path is replaced, never written through, so a link at that name cannot reach the user's file.
fn write_private(path: &Path, text: &str) -> Result<()> {
    use std::io::Write;
    use std::os::unix::fs::OpenOptionsExt;
    let tmp = temp_sibling(path);
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .mode(0o600)
        .open(&tmp)
        .with_context(|| format!("writing {}", tmp.display()))?;
    file.write_all(text.as_bytes())?;
    std::fs::rename(&tmp, path).with_context(|| format!("replacing {}", path.display()))
}

/// The user's `hooks.json` with Lotse's handler appended to every event, and the group index each
/// of ours got (the user's groups keep theirs).
fn build_hooks(user: Option<&str>) -> Result<(String, Vec<(&'static str, usize)>)> {
    let mut root = match user.filter(|t| !t.trim().is_empty()) {
        Some(text) => {
            serde_json::from_str::<Value>(text).context("hooks.json is not valid JSON")?
        }
        None => json!({}),
    };
    let Some(top) = root.as_object_mut() else {
        bail!("hooks.json is not a JSON object");
    };
    let Some(events) = top
        .entry("hooks")
        .or_insert_with(|| json!({}))
        .as_object_mut()
    else {
        bail!("hooks.json: \"hooks\" is not an object");
    };
    let mut groups = Vec::new();
    for (event, label) in EVENTS {
        let Some(list) = events
            .entry(event)
            .or_insert_with(|| json!([]))
            .as_array_mut()
        else {
            bail!("hooks.json: \"{event}\" is not an array");
        };
        groups.push((label, list.len()));
        list.push(json!({ "hooks": [{
            "type": "command",
            "command": HOOK_COMMAND,
            "timeout": timeout_seconds(label),
        }] }));
    }
    Ok((serde_json::to_string_pretty(&root)? + "\n", groups))
}

struct Paths {
    /// The user's home as given, and with symlinks resolved: Codex may have recorded either.
    source: PathBuf,
    canonical_source: PathBuf,
    /// The mirror, symlinks resolved.
    mirror: PathBuf,
}

/// The user's `config.toml` plus the trust entries: one per Lotse hook, and the user's own hook
/// trust under the mirror's paths. Refuses what it cannot add to without breaking the file.
fn build_config(user: &str, paths: &Paths, groups: &[(&'static str, usize)]) -> Result<String> {
    let table: toml::Table = toml::from_str(user).context("config.toml is not valid TOML")?;
    let hooks_json = paths.mirror.join(HOOKS_FILE);
    let mut blocks = Vec::new();
    let mut ours = BTreeSet::new();
    for &(label, group) in groups {
        let key = format!("{}:{label}:{group}:0", hooks_json.display());
        let hash = trusted_hash(label, HOOK_COMMAND, timeout_seconds(label));
        blocks.push(trust_block(&key, Some(true), Some(&hash)));
        ours.insert(key);
    }
    // The user's own hooks keep working: their trust is keyed by the real files' paths.
    let state = table
        .get("hooks")
        .and_then(toml::Value::as_table)
        .and_then(|h| h.get("state"))
        .and_then(toml::Value::as_table);
    for (key, entry) in state.into_iter().flatten() {
        for file in [HOOKS_FILE, CONFIG_FILE] {
            for home in [&paths.source, &paths.canonical_source] {
                let prefix = format!("{}:", home.join(file).display());
                let Some(rest) = key.strip_prefix(&prefix) else {
                    continue;
                };
                let moved = format!("{}:{rest}", paths.mirror.join(file).display());
                if ours.insert(moved.clone()) {
                    blocks.push(trust_block(
                        &moved,
                        entry.get("enabled").and_then(toml::Value::as_bool),
                        entry.get("trusted_hash").and_then(toml::Value::as_str),
                    ));
                }
            }
        }
    }
    let mut config = user.to_owned();
    if daemon_socket_too_long(&paths.mirror) && !daemon_auto_start_set(&table) {
        config = turn_daemon_off(&config);
    }
    if !config.is_empty() && !config.ends_with('\n') {
        config.push('\n');
    }
    if !config.is_empty() {
        config.push('\n');
    }
    config.push_str(&blocks.join("\n\n"));
    config.push('\n');
    toml::from_str::<toml::Table>(&config)
        .context("cannot add the Lotse hook trust to this config.toml without breaking it")?;
    Ok(config)
}

fn trust_block(key: &str, enabled: Option<bool>, hash: Option<&str>) -> String {
    let mut block = format!("[hooks.state.{}]", toml_string(key));
    if let Some(enabled) = enabled {
        block.push_str(&format!("\nenabled = {enabled}"));
    }
    if let Some(hash) = hash {
        block.push_str(&format!("\ntrusted_hash = {}", toml_string(hash)));
    }
    block
}

fn toml_string(value: &str) -> String {
    let mut out = String::from('"');
    for c in value.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if c.is_control() => out.push_str(&format!("\\u{:04x}", u32::from(c))),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

fn daemon_socket_too_long(mirror: &Path) -> bool {
    mirror.join(DAEMON_SOCKET).as_os_str().len() >= SOCKET_PATH_LIMIT
}

fn daemon_auto_start_set(config: &toml::Table) -> bool {
    config
        .get("features")
        .and_then(toml::Value::as_table)
        .is_some_and(|f| f.contains_key("daemon_auto_start"))
}

/// Set `features.daemon_auto_start = false` the way Orca does for long homes
/// (`src/main/codex/codex-daemon-socket-path-guard.ts`): inside the user's `[features]` table when
/// they have one, else in a new one. The caller checks the result still parses.
fn turn_daemon_off(config: &str) -> String {
    let setting = format!("daemon_auto_start = false {DAEMON_OFF_COMMENT}");
    let mut lines: Vec<&str> = config.lines().collect();
    let header = lines.iter().position(|l| {
        let l = l.trim();
        l.strip_prefix("[features]")
            .is_some_and(|rest| rest.trim().is_empty() || rest.trim_start().starts_with('#'))
    });
    match header {
        Some(at) => {
            lines.insert(at + 1, &setting);
            lines.join("\n") + "\n"
        }
        None => format!("{config}\n[features]\n{setting}\n"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::symlink;
    use std::time::Duration;

    struct Homes {
        _tmp: tempfile::TempDir,
        root: PathBuf,
        source: PathBuf,
    }

    fn homes() -> Homes {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("data/integrations/codex");
        let source = tmp.path().join("user/.codex");
        std::fs::create_dir_all(&source).unwrap();
        Homes {
            _tmp: tmp,
            root,
            source,
        }
    }

    fn put(path: &Path, text: &str) {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, text).unwrap();
    }

    fn read(path: &Path) -> String {
        std::fs::read_to_string(path).unwrap()
    }

    fn set_mtime(path: &Path, secs: u64) {
        let file = std::fs::OpenOptions::new().write(true).open(path).unwrap();
        file.set_modified(UNIX_EPOCH + Duration::from_secs(secs))
            .unwrap();
    }

    fn config_of(mirror: &Path) -> toml::Table {
        toml::from_str(&read(&mirror.join("config.toml"))).unwrap()
    }

    fn state_of(mirror: &Path) -> toml::Table {
        config_of(mirror)["hooks"]["state"]
            .as_table()
            .unwrap()
            .clone()
    }

    /// `currentHash` of each hook as reported by Codex 0.162.1 (`app-server`, `hooks/list`) for a
    /// `hooks.json` with this command, recorded on 2026-11-02 in a scratch `CODEX_HOME`.
    const CODEX_0_162_1_HASHES: [(&str, &str); 9] = [
        (
            "session_start",
            "sha256:3c95ea3bd0d7d0ee1e9f351125b0f42712264407419509fab9ac4e31d6853077",
        ),
        (
            "user_prompt_submit",
            "sha256:1e31157ddedda01211c9f20adc3304ab597133aa2421cb691da32b1a02ee02a0",
        ),
        (
            "pre_tool_use",
            "sha256:401df874428464e29adca97994266282d79e087d8d55beeccae04c845e7c8af4",
        ),
        (
            "permission_request",
            "sha256:aafb09afd935ffd4093dfc81cbac29d88f65b5ff403b380509376d2d24c203b2",
        ),
        (
            "post_tool_use",
            "sha256:bc48e2f45c853aae32e9e0560002fb6887093541090523e54dc6a1e6fc684427",
        ),
        (
            "subagent_start",
            "sha256:1b9250c867e10c62ad85c725751cdd5dffbb07b31de069d5fab8297ea44d0546",
        ),
        (
            "subagent_stop",
            "sha256:6cfdf1640c05909c43dbb430788c7a73b1cf0fb78391679a553ffe275b4af8bb",
        ),
        (
            "stop",
            "sha256:71aacb6faaa1969c5cd5b148e41860d38e9b8fe7d6309d459691e278b84bcadd",
        ),
        (
            "interrupt",
            "sha256:0d76a2153cb8bdca51b1a62a228bc7a5e5ba75a459f74de8b57830d5b18bb05a",
        ),
    ];

    /// The golden table is the one of exactly the events Lotse hooks: adding or dropping an event
    /// without re-verifying its hash against `VERIFIED_CODEX_VERSION` fails here, and the hashes of
    /// the mirror's `config.toml` are those of the table, not recomputed text.
    #[test]
    fn the_pinned_codex_version_covers_exactly_the_hooked_events() {
        assert_eq!(VERIFIED_CODEX_VERSION, "0.162.1");
        let mut golden: Vec<&str> = CODEX_0_162_1_HASHES.iter().map(|(l, _)| *l).collect();
        let mut hooked: Vec<&str> = EVENTS.iter().map(|(_, l)| *l).collect();
        golden.sort_unstable();
        hooked.sort_unstable();
        assert_eq!(golden, hooked);
        let h = homes();
        let mirror = mirror(&h.root, &h.source).unwrap();
        let config = read(&mirror.join("config.toml"));
        for (label, hash) in CODEX_0_162_1_HASHES {
            assert!(
                config.contains(&format!("trusted_hash = \"{hash}\"")),
                "{label}: the mirror's config lacks the hash Codex {VERIFIED_CODEX_VERSION} reports"
            );
        }
    }

    #[test]
    fn trusted_hash_is_the_hash_codex_reports_for_the_hook() {
        for (event, label) in EVENTS {
            let expected = CODEX_0_162_1_HASHES
                .iter()
                .find(|(l, _)| *l == label)
                .unwrap_or_else(|| panic!("no recorded hash for {event}"))
                .1;
            assert_eq!(
                trusted_hash(label, HOOK_COMMAND, timeout_seconds(label)),
                expected,
                "{event}"
            );
        }
    }

    #[test]
    fn hooks_json_is_the_users_hooks_then_ours_for_every_event() {
        let h = homes();
        put(
            &h.source.join("hooks.json"),
            r#"{"hooks":{"PreToolUse":[{"matcher":"Bash","hooks":[{"type":"command","command":"echo mine"}]}],"Custom":[]}}"#,
        );
        let mirror = mirror(&h.root, &h.source).unwrap();
        let hooks: Value = serde_json::from_str(&read(&mirror.join("hooks.json"))).unwrap();
        let events = hooks["hooks"].as_object().unwrap();

        // the user's group stays first, with its index; ours is after it
        let pre = events["PreToolUse"].as_array().unwrap();
        assert_eq!(pre.len(), 2);
        assert_eq!(pre[0]["hooks"][0]["command"], "echo mine");
        assert_eq!(pre[1]["hooks"][0]["command"], HOOK_COMMAND);
        assert!(events["Custom"].as_array().unwrap().is_empty());
        for (event, label) in EVENTS {
            let last = events[event].as_array().unwrap().last().unwrap();
            assert_eq!(last["hooks"][0]["type"], "command", "{event}");
            assert_eq!(last["hooks"][0]["command"], HOOK_COMMAND, "{event}");
            assert_eq!(
                last["hooks"][0]["timeout"],
                timeout_seconds(label),
                "{event}"
            );
            assert!(last.get("matcher").is_none(), "{event}");
        }
        assert_eq!(events.len(), EVENTS.len() + 1);
    }

    #[test]
    fn config_toml_keeps_the_users_settings_and_trusts_each_hook_at_the_mirror_path() {
        let h = homes();
        put(
            &h.source.join("config.toml"),
            "model = \"gpt-5\"\n\n[projects.\"/work\"]\ntrust_level = \"trusted\"\n",
        );
        put(&h.source.join("hooks.json"), "{}");
        let before = (
            read(&h.source.join("config.toml")),
            read(&h.source.join("hooks.json")),
        );
        let mirror = mirror(&h.root, &h.source).unwrap();

        let config = config_of(&mirror);
        assert_eq!(config["model"].as_str(), Some("gpt-5"));
        assert_eq!(
            config["projects"]["/work"]["trust_level"].as_str(),
            Some("trusted")
        );
        let canonical = std::fs::canonicalize(&mirror).unwrap();
        let state = state_of(&mirror);
        assert_eq!(state.len(), EVENTS.len());
        for (_, label) in EVENTS {
            let key = format!("{}/hooks.json:{label}:0:0", canonical.display());
            assert_eq!(state[&key]["enabled"].as_bool(), Some(true), "{key}");
            assert_eq!(
                state[&key]["trusted_hash"].as_str(),
                Some(trusted_hash(label, HOOK_COMMAND, timeout_seconds(label)).as_str()),
                "{key}"
            );
        }
        // the user's files are not touched
        assert_eq!(read(&h.source.join("config.toml")), before.0);
        assert_eq!(read(&h.source.join("hooks.json")), before.1);
    }

    #[test]
    fn the_users_hook_trust_moves_to_the_mirror_and_never_shadows_ours() {
        let h = homes();
        let src = std::fs::canonicalize(&h.source).unwrap();
        put(
            &h.source.join("hooks.json"),
            r#"{"hooks":{"PreToolUse":[{"hooks":[{"type":"command","command":"echo mine"}]}]}}"#,
        );
        put(
            &h.source.join("config.toml"),
            &format!(
                "[hooks.state.\"{s}/hooks.json:pre_tool_use:0:0\"]\nenabled = false\ntrusted_hash = \"sha256:mine\"\n\n\
                 [hooks.state.\"{s}/config.toml:stop:0:0\"]\ntrusted_hash = \"sha256:inline\"\n\n\
                 [hooks.state.\"{s}/hooks.json:stop:0:0\"]\ntrusted_hash = \"sha256:stale\"\n\n\
                 [hooks.state.\"/elsewhere/hooks.json:stop:0:0\"]\ntrusted_hash = \"sha256:foreign\"\n",
                s = src.display()
            ),
        );
        let mirror = mirror(&h.root, &h.source).unwrap();
        let canonical = std::fs::canonicalize(&mirror).unwrap();
        let state = state_of(&mirror);
        let at = |file: &str, rest: &str| &state[&format!("{}/{file}:{rest}", canonical.display())];

        let carried = at("hooks.json", "pre_tool_use:0:0");
        assert_eq!(carried["trusted_hash"].as_str(), Some("sha256:mine"));
        assert_eq!(carried["enabled"].as_bool(), Some(false));
        assert_eq!(
            at("config.toml", "stop:0:0")["trusted_hash"].as_str(),
            Some("sha256:inline")
        );
        // Lotse's own PreToolUse sits at group 1, after the user's
        assert_eq!(
            at("hooks.json", "pre_tool_use:1:0")["trusted_hash"].as_str(),
            Some(trusted_hash("pre_tool_use", HOOK_COMMAND, 10).as_str())
        );
        // a stale trust entry for the slot Lotse took does not win over Lotse's
        assert_eq!(
            at("hooks.json", "stop:0:0")["trusted_hash"].as_str(),
            Some(trusted_hash("stop", HOOK_COMMAND, 10).as_str())
        );
        // keys of other files are not ours to move
        assert!(
            !state
                .keys()
                .any(|k| k.contains("/elsewhere/")
                    && k.starts_with(&canonical.display().to_string()))
        );
    }

    #[test]
    fn everything_else_is_a_link_and_writes_go_through_to_the_real_home() {
        let h = homes();
        put(&h.source.join("auth.json"), "{\"t\":1}");
        put(&h.source.join("history.jsonl"), "one\n");
        put(&h.source.join("sessions/2026/a.jsonl"), "s\n");
        let mirror = mirror(&h.root, &h.source).unwrap();

        for name in ["auth.json", "history.jsonl", "sessions"] {
            assert_eq!(
                std::fs::read_link(mirror.join(name)).unwrap(),
                h.source.join(name)
            );
        }
        for name in ["hooks.json", "config.toml"] {
            assert!(!is_link(&mirror.join(name)), "{name} must be a real file");
        }
        // an in-place write (what Codex does for history and rollouts) is the real file's
        let mut history = std::fs::OpenOptions::new()
            .append(true)
            .open(mirror.join("history.jsonl"))
            .unwrap();
        std::io::Write::write_all(&mut history, b"two\n").unwrap();
        put(&mirror.join("sessions/2026/b.jsonl"), "new\n");
        assert_eq!(read(&h.source.join("history.jsonl")), "one\ntwo\n");
        assert_eq!(read(&h.source.join("sessions/2026/b.jsonl")), "new\n");
    }

    #[test]
    fn auth_json_that_codex_replaced_by_rename_lands_in_the_real_file() {
        let h = homes();
        put(&h.source.join("auth.json"), "{\"refresh\":\"old\"}");
        set_mtime(&h.source.join("auth.json"), 1_000);
        let mirror = mirror(&h.root, &h.source).unwrap();

        // Codex refreshes the token by writing a temp file and renaming it over `auth.json`,
        // which swaps the link for a file
        put(&mirror.join(".auth.json.tmp"), "{\"refresh\":\"new\"}");
        std::fs::rename(mirror.join(".auth.json.tmp"), mirror.join("auth.json")).unwrap();
        assert!(!is_link(&mirror.join("auth.json")));
        assert_eq!(read(&h.source.join("auth.json")), "{\"refresh\":\"old\"}");

        super::mirror(&h.root, &h.source).unwrap();
        assert_eq!(read(&h.source.join("auth.json")), "{\"refresh\":\"new\"}");
        assert_eq!(
            std::fs::read_link(mirror.join("auth.json")).unwrap(),
            h.source.join("auth.json")
        );
        assert!(!std::fs::read_dir(&h.source).unwrap().any(|e| {
            e.unwrap()
                .file_name()
                .to_string_lossy()
                .contains("lotse-tmp")
        }));
    }

    #[test]
    fn a_real_auth_json_that_changed_since_wins_over_a_stale_mirror_copy() {
        let h = homes();
        put(&h.source.join("auth.json"), "{\"login\":\"first\"}");
        let mirror = mirror(&h.root, &h.source).unwrap();
        std::fs::remove_file(mirror.join("auth.json")).unwrap();
        put(&mirror.join("auth.json"), "{\"login\":\"stale\"}");
        set_mtime(&mirror.join("auth.json"), 1_000);
        // the user logged in again outside Lotse, later
        put(&h.source.join("auth.json"), "{\"login\":\"second\"}");
        set_mtime(&h.source.join("auth.json"), 2_000);

        super::mirror(&h.root, &h.source).unwrap();
        assert_eq!(read(&h.source.join("auth.json")), "{\"login\":\"second\"}");
        assert!(is_link(&mirror.join("auth.json")));
    }

    #[test]
    fn a_login_made_in_the_mirror_is_kept_when_the_real_home_had_none() {
        let h = homes();
        let mirror = mirror(&h.root, &h.source).unwrap();
        put(&mirror.join("auth.json"), "{\"login\":\"new\"}");
        put(&mirror.join("sessions/x/r.jsonl"), "r\n");

        super::mirror(&h.root, &h.source).unwrap();
        assert_eq!(read(&h.source.join("auth.json")), "{\"login\":\"new\"}");
        assert_eq!(read(&h.source.join("sessions/x/r.jsonl")), "r\n");
        assert!(is_link(&mirror.join("auth.json")));
        assert!(is_link(&mirror.join("sessions")));
    }

    #[test]
    fn an_entry_removed_from_the_real_home_leaves_the_mirror() {
        let h = homes();
        put(&h.source.join("gone.json"), "{}");
        let mirror = mirror(&h.root, &h.source).unwrap();
        assert!(is_link(&mirror.join("gone.json")));
        std::fs::remove_file(h.source.join("gone.json")).unwrap();
        super::mirror(&h.root, &h.source).unwrap();
        assert!(std::fs::symlink_metadata(mirror.join("gone.json")).is_err());
    }

    #[test]
    fn the_generated_files_follow_the_users_files_by_stamp_and_only_then() {
        let h = homes();
        put(&h.source.join("config.toml"), "model = \"a\"\n");
        set_mtime(&h.source.join("config.toml"), 1_000);
        let mirror = mirror(&h.root, &h.source).unwrap();
        assert_eq!(config_of(&mirror)["model"].as_str(), Some("a"));

        // Codex records a choice in the mirror's config: a second pane must not discard it
        put(&mirror.join("config.toml"), "# chosen in a pane\n");
        super::mirror(&h.root, &h.source).unwrap();
        assert_eq!(read(&mirror.join("config.toml")), "# chosen in a pane\n");

        // the user edits their config
        put(&h.source.join("config.toml"), "model = \"b\"\n");
        set_mtime(&h.source.join("config.toml"), 2_000);
        super::mirror(&h.root, &h.source).unwrap();
        assert_eq!(config_of(&mirror)["model"].as_str(), Some("b"));

        // and a hooks.json appears
        put(
            &h.source.join("hooks.json"),
            r#"{"hooks":{"Stop":[{"hooks":[{"type":"command","command":"x"}]}]}}"#,
        );
        super::mirror(&h.root, &h.source).unwrap();
        assert_eq!(
            serde_json::from_str::<Value>(&read(&mirror.join("hooks.json"))).unwrap()["hooks"]["Stop"]
                .as_array()
                .unwrap()
                .len(),
            2
        );
        assert!(
            state_of(&mirror)
                .keys()
                .any(|k| k.ends_with("/hooks.json:stop:1:0"))
        );
    }

    #[test]
    fn one_mirror_per_source_home() {
        let h = homes();
        let other = h.source.with_file_name(".codex-work");
        std::fs::create_dir_all(&other).unwrap();
        let a = mirror(&h.root, &h.source).unwrap();
        assert_eq!(mirror(&h.root, &h.source).unwrap(), a);
        let b = mirror(&h.root, &other).unwrap();
        assert_ne!(a, b);
        assert!(a.starts_with(&h.root) && b.starts_with(&h.root));
    }

    #[test]
    fn what_cannot_be_mirrored_is_an_error_and_the_source_stays_as_it_was() {
        let h = homes();
        assert!(mirror(&h.root, &h.source.join("missing")).is_err());

        put(&h.source.join("hooks.json"), "{ not json");
        let err = mirror(&h.root, &h.source).unwrap_err();
        assert!(format!("{err:#}").contains("hooks.json"), "{err:#}");
        put(&h.source.join("hooks.json"), "[]");
        assert!(mirror(&h.root, &h.source).is_err());

        std::fs::remove_file(h.source.join("hooks.json")).unwrap();
        put(&h.source.join("config.toml"), "model = ");
        let err = mirror(&h.root, &h.source).unwrap_err();
        assert!(format!("{err:#}").contains("config.toml"), "{err:#}");
        assert_eq!(read(&h.source.join("config.toml")), "model = ");
    }

    #[test]
    fn a_mirror_path_too_long_for_the_codex_daemon_socket_turns_its_auto_start_off() {
        let tmp = tempfile::tempdir().unwrap();
        let source = tmp.path().join("user/.codex");
        std::fs::create_dir_all(&source).unwrap();
        let long = tmp.path().join("x".repeat(120)).join("codex");
        put(
            &source.join("config.toml"),
            "[features]\nfoo = true # keep\n\n[tui]\nbar = 1\n",
        );
        let mirror = mirror(&long, &source).unwrap();
        let features = config_of(&mirror)["features"].as_table().unwrap().clone();
        assert_eq!(features["daemon_auto_start"].as_bool(), Some(false));
        assert_eq!(features["foo"].as_bool(), Some(true));
        assert_eq!(config_of(&mirror)["tui"]["bar"].as_integer(), Some(1));

        // without a [features] table of its own, one is added; the user's own choice is kept
        put(&source.join("config.toml"), "model = \"m\"\n");
        let again = super::mirror(&long, &source).unwrap();
        assert_eq!(
            config_of(&again)["features"]["daemon_auto_start"].as_bool(),
            Some(false)
        );
        put(
            &source.join("config.toml"),
            "[features]\ndaemon_auto_start = true\n",
        );
        set_mtime(&source.join("config.toml"), 5_000);
        let kept = super::mirror(&long, &source).unwrap();
        assert_eq!(
            config_of(&kept)["features"]["daemon_auto_start"].as_bool(),
            Some(true)
        );

        // the limit is the socket path's length: a short home needs no override
        assert!(!daemon_socket_too_long(Path::new("/tmp/lotse-codex")));
        assert!(daemon_socket_too_long(&long));
    }

    #[test]
    fn a_real_home_that_is_a_link_is_followed() {
        let h = homes();
        let real = h.source.with_file_name("dotfiles-codex");
        std::fs::create_dir_all(&real).unwrap();
        put(&real.join("auth.json"), "{}");
        std::fs::remove_dir(&h.source).unwrap();
        symlink(&real, &h.source).unwrap();
        let mirror = mirror(&h.root, &h.source).unwrap();
        assert_eq!(read(&mirror.join("auth.json")), "{}");
    }
}
