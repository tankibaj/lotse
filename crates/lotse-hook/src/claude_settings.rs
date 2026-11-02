//! Installer and uninstaller for the Lotse hooks in Claude Code's user `settings.json`.
//!
//! What gets installed is listed in `integrations/claude/hooks.json` (embedded at build time): one
//! `command` hook per event that runs `lotse-hook claude`. Settings shape (hooks reference,
//! <https://code.claude.com/docs/en/hooks>, **Unverified** against the live page):
//! `{"hooks": {"<Event>": [{"matcher": "<pattern>", "hooks": [{"type": "command", "command": "...", "timeout": <seconds>}]}]}}`.
//!
//! * **Marker.** Every command ends in a shell comment, `# lotse:managed`. Install and uninstall only
//!   ever touch handlers whose command contains it; the user's own hooks (even in the same group) stay.
//! * **Idempotent.** Installing twice changes nothing; a moved binary updates the command in place.
//! * **Backup.** Before the first change to a file that has no Lotse hooks yet, its exact bytes are
//!   copied to `settings.json.lotse-backup` (mode 0600; it can hold secrets, like the file itself).
//! * **Round trip.** Uninstall removes the marked handlers (and the groups, events and `hooks` object
//!   that were left empty). When the result is the same JSON as the backup, the backup's bytes are
//!   restored, so install → uninstall leaves the file byte-identical even for unusual formatting. If
//!   the user changed the file in between, only the Lotse handlers are removed and the edits stay.
//! * **Safe.** A file that is not strict JSON, or whose `hooks` has an unexpected shape, is left
//!   untouched and reported as an error. Writes go through a temp file and `rename`, a symlinked
//!   `settings.json` is written through, and permissions are kept.
//!
//! Nothing here reads the environment: callers pass the config directory (`CLAUDE_CONFIG_DIR` or
//! `~/.claude`, see [`config_dir`]), so tests run against a temp directory.

use crate::json::{self, Json};
use anyhow::{Context, Result, bail};
use std::ffi::OsStr;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

const TEMPLATE: &str = include_str!("../../../integrations/claude/hooks.json");

/// The hooks Lotse installs (parsed `integrations/claude/hooks.json`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Template {
    /// Trailing shell comment that marks a command as ours.
    pub marker: String,
    pub timeout_seconds: u64,
    pub events: Vec<EventSpec>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EventSpec {
    pub event: String,
    pub matcher: Option<String>,
}

pub fn template() -> Result<Template> {
    let value: serde_json::Value = serde_json::from_str(TEMPLATE)
        .context("integrations/claude/hooks.json is not valid JSON")?;
    let marker = value["marker"]
        .as_str()
        .filter(|m| !m.is_empty())
        .context("hooks.json: missing `marker`")?
        .to_owned();
    let timeout_seconds = value["timeout_seconds"]
        .as_u64()
        .context("hooks.json: missing `timeout_seconds`")?;
    let events = value["events"]
        .as_array()
        .context("hooks.json: missing `events`")?
        .iter()
        .map(|e| {
            Ok(EventSpec {
                event: e["event"]
                    .as_str()
                    .context("hooks.json: event without a name")?
                    .to_owned(),
                matcher: e["matcher"].as_str().map(str::to_owned),
            })
        })
        .collect::<Result<Vec<_>>>()?;
    Ok(Template {
        marker,
        timeout_seconds,
        events,
    })
}

/// Claude's config directory: `$CLAUDE_CONFIG_DIR` when set and non-empty, else `<home>/.claude`.
pub fn config_dir(claude_config_dir: Option<&OsStr>, home: Option<&Path>) -> Option<PathBuf> {
    match claude_config_dir.filter(|d| !d.is_empty()) {
        Some(dir) => Some(PathBuf::from(dir)),
        None => home.map(|h| h.join(".claude")),
    }
}

pub fn settings_path(config_dir: &Path) -> PathBuf {
    config_dir.join("settings.json")
}

/// Where the pre-install bytes of `settings.json` are kept.
pub fn backup_path(config_dir: &Path) -> PathBuf {
    backup_path_for(&write_target(&settings_path(config_dir)))
}

/// The shell command a hook runs: `'<hook binary>' claude # lotse:managed`.
pub fn hook_command(hook_path: &Path, marker: &str) -> Result<String> {
    let path = hook_path
        .to_str()
        .context("the lotse-hook path is not valid UTF-8")?;
    Ok(format!("{} claude {marker}", shell_quote(path)))
}

fn shell_quote(text: &str) -> String {
    format!("'{}'", text.replace('\'', "'\\''"))
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InstallOutcome {
    /// Everything was already in place; no file was written.
    Unchanged,
    /// `settings.json` was written. `backup` is set when the original bytes were saved.
    Written { backup: Option<PathBuf> },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UninstallOutcome {
    /// No Lotse hooks found; no file was written.
    NotInstalled,
    /// The Lotse hooks were removed; every other setting was kept.
    Removed,
    /// The file equalled the backup once the hooks were removed, so the backup's bytes came back.
    RestoredBackup,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InstallStatus {
    NotInstalled,
    Installed,
    /// Some of the events have no Lotse hook.
    Partial {
        missing: Vec<String>,
    },
}

/// Add the Lotse hooks to `<config_dir>/settings.json` (created when absent).
pub fn install(config_dir: &Path, hook_path: &Path) -> Result<InstallOutcome> {
    let tpl = template()?;
    let command = hook_command(hook_path, &tpl.marker)?;
    let target = write_target(&settings_path(config_dir));
    let loaded = load(&target)?;

    let mut tree = loaded.tree.clone();
    ensure_hooks(&mut tree, &tpl, &command)?;
    if loaded.text.is_some() && tree == loaded.tree {
        return Ok(InstallOutcome::Unchanged);
    }

    fs::create_dir_all(config_dir).with_context(|| format!("creating {}", config_dir.display()))?;
    let mut backup = None;
    if let Some(original) = &loaded.text {
        // A file that already has our hooks is an upgrade: keep the first (pre-Lotse) backup.
        if !has_marked(&loaded.tree, &tpl.marker) {
            let path = backup_path_for(&target);
            write_private(&path, original.as_bytes())?;
            backup = Some(path);
        }
    }
    let text = render(&tree, &loaded.style);
    write_atomic(&target, text.as_bytes())?;
    Ok(InstallOutcome::Written { backup })
}

/// Remove the Lotse hooks from `<config_dir>/settings.json`.
pub fn uninstall(config_dir: &Path) -> Result<UninstallOutcome> {
    let tpl = template()?;
    let settings = settings_path(config_dir);
    if !settings.exists() {
        return Ok(UninstallOutcome::NotInstalled);
    }
    let target = write_target(&settings);
    let loaded = load(&target)?;

    let mut pruned = loaded.tree.clone();
    if !strip_managed(&mut pruned, &tpl.marker, None) {
        return Ok(UninstallOutcome::NotInstalled);
    }
    // Install may have created the `hooks` object and event arrays, or filled ones that already
    // existed empty. The backup shows which, so empty containers it has are kept when comparing.
    if let Ok(saved) = fs::read_to_string(backup_path_for(&target)) {
        let saved_tree = if saved.trim().is_empty() {
            Some(Json::Obj(Vec::new()))
        } else {
            json::parse(&saved).ok()
        };
        if let Some(saved_tree) = saved_tree {
            let mut like_backup = loaded.tree.clone();
            strip_managed(&mut like_backup, &tpl.marker, Some(&saved_tree));
            if like_backup == saved_tree {
                write_atomic(&target, saved.as_bytes())?;
                return Ok(UninstallOutcome::RestoredBackup);
            }
        }
    }
    write_atomic(&target, render(&pruned, &loaded.style).as_bytes())?;
    Ok(UninstallOutcome::Removed)
}

/// Which of the template's events have a Lotse hook in `<config_dir>/settings.json`.
pub fn status(config_dir: &Path) -> Result<InstallStatus> {
    let tpl = template()?;
    let settings = settings_path(config_dir);
    if !settings.exists() {
        return Ok(InstallStatus::NotInstalled);
    }
    let loaded = load(&write_target(&settings))?;
    let missing: Vec<String> = tpl
        .events
        .iter()
        .filter(|spec| {
            loaded
                .tree
                .get("hooks")
                .and_then(|h| h.get(&spec.event))
                .is_none_or(|groups| count_marked(groups, &tpl.marker) == 0)
        })
        .map(|spec| spec.event.clone())
        .collect();
    Ok(if missing.is_empty() {
        InstallStatus::Installed
    } else if missing.len() == tpl.events.len() {
        InstallStatus::NotInstalled
    } else {
        InstallStatus::Partial { missing }
    })
}

// ---- tree editing ---------------------------------------------------------------------------

/// Make sure every template event has exactly one Lotse hook, appending it as its own group.
fn ensure_hooks(tree: &mut Json, tpl: &Template, command: &str) -> Result<()> {
    let Json::Obj(members) = tree else {
        bail!("settings.json must contain a JSON object");
    };
    if tree_get(members, "hooks").is_none() {
        members.push((json::encode("hooks"), Json::Obj(Vec::new())));
    }
    let Some(Json::Obj(events)) = member_mut(members, "hooks") else {
        bail!("`hooks` in settings.json is not an object; not touching it");
    };
    for spec in &tpl.events {
        if tree_get(events, &spec.event).is_none() {
            events.push((json::encode(&spec.event), Json::Arr(Vec::new())));
        }
        let Some(Json::Arr(groups)) = member_mut(events, &spec.event) else {
            bail!(
                "`hooks.{}` in settings.json is not an array; not touching it",
                spec.event
            );
        };
        let canonical = canonical_group(spec, command, tpl.timeout_seconds);
        let marked: usize = groups.iter().map(|g| marked_in_group(g, &tpl.marker)).sum();
        if marked == 1 && groups.contains(&canonical) {
            continue;
        }
        remove_marked(groups, &tpl.marker);
        groups.push(canonical);
    }
    Ok(())
}

fn canonical_group(spec: &EventSpec, command: &str, timeout: u64) -> Json {
    let handler = Json::object([
        ("type", Json::string("command")),
        ("command", Json::string(command)),
        ("timeout", Json::Num(timeout.to_string())),
    ]);
    let mut members = Vec::new();
    if let Some(matcher) = &spec.matcher {
        members.push((json::encode("matcher"), Json::string(matcher)));
    }
    members.push((json::encode("hooks"), Json::Arr(vec![handler])));
    Json::Obj(members)
}

fn tree_get<'a>(members: &'a [(String, Json)], key: &str) -> Option<&'a Json> {
    members
        .iter()
        .find(|(k, _)| json::decode(k) == key)
        .map(|(_, v)| v)
}

fn member_mut<'a>(members: &'a mut [(String, Json)], key: &str) -> Option<&'a mut Json> {
    members
        .iter_mut()
        .find(|(k, _)| json::decode(k) == key)
        .map(|(_, v)| v)
}

fn is_marked(handler: &Json, marker: &str) -> bool {
    handler
        .get("command")
        .and_then(Json::as_string)
        .is_some_and(|c| c.contains(marker))
}

fn marked_in_group(group: &Json, marker: &str) -> usize {
    match group.get("hooks") {
        Some(Json::Arr(handlers)) => handlers.iter().filter(|h| is_marked(h, marker)).count(),
        _ => 0,
    }
}

/// Number of marked handlers in an event's group list (0 when it is not a list).
fn count_marked(groups: &Json, marker: &str) -> usize {
    match groups {
        Json::Arr(groups) => groups.iter().map(|g| marked_in_group(g, marker)).sum(),
        _ => 0,
    }
}

fn has_marked(tree: &Json, marker: &str) -> bool {
    match tree.get("hooks") {
        Some(Json::Obj(events)) => events.iter().any(|(_, v)| count_marked(v, marker) > 0),
        _ => false,
    }
}

/// Drop marked handlers from a group list, and the groups that this leaves empty. Returns how many
/// handlers went. Groups that were already empty, and anything unrecognised, stay.
fn remove_marked(groups: &mut Vec<Json>, marker: &str) -> usize {
    let mut removed = 0;
    groups.retain_mut(|group| {
        let Some(Json::Arr(handlers)) = group.get_mut("hooks") else {
            return true;
        };
        let before = handlers.len();
        handlers.retain(|h| !is_marked(h, marker));
        let gone = before - handlers.len();
        removed += gone;
        !(gone > 0 && handlers.is_empty())
    });
    removed
}

/// Remove every marked handler from `tree`, then the event arrays and the `hooks` object this
/// leaves empty, except those that `reference` (the pre-install file) has. Returns whether any
/// handler was removed.
fn strip_managed(tree: &mut Json, marker: &str, reference: Option<&Json>) -> bool {
    let Json::Obj(members) = tree else {
        return false;
    };
    let Some(Json::Obj(events)) = member_mut(members, "hooks") else {
        return false;
    };
    let reference_hooks = reference.and_then(|r| r.get("hooks"));
    let mut changed = false;
    events.retain_mut(|(key, groups)| {
        let Json::Arr(groups) = groups else {
            return true;
        };
        if remove_marked(groups, marker) == 0 {
            return true;
        }
        changed = true;
        let existed = reference_hooks.is_some_and(|h| h.get(&json::decode(key)).is_some());
        !groups.is_empty() || existed
    });
    if changed && events.is_empty() && reference_hooks.is_none() {
        members.retain(|(k, _)| json::decode(k) != "hooks");
    }
    changed
}

// ---- files ----------------------------------------------------------------------------------

struct Style {
    indent: String,
    eol: &'static str,
    final_newline: bool,
}

impl Default for Style {
    fn default() -> Self {
        Style {
            indent: "  ".into(),
            eol: "\n",
            final_newline: true,
        }
    }
}

impl Style {
    /// Indent unit, line ending and final newline of an existing file.
    fn detect(text: &str) -> Style {
        let indent = text
            .lines()
            .skip(1)
            .find_map(|line| {
                let body = line.trim_start_matches([' ', '\t']);
                (body.starts_with('"') && body.len() < line.len())
                    .then(|| line[..line.len() - body.len()].to_owned())
            })
            .unwrap_or_else(|| "  ".into());
        Style {
            indent,
            eol: if text.contains("\r\n") { "\r\n" } else { "\n" },
            final_newline: text.ends_with('\n'),
        }
    }
}

fn render(tree: &Json, style: &Style) -> String {
    let mut text = json::to_string(tree, &style.indent, style.eol);
    if style.final_newline {
        text.push_str(style.eol);
    }
    text
}

struct Loaded {
    /// File content; `None` when the file does not exist.
    text: Option<String>,
    tree: Json,
    style: Style,
}

fn load(path: &Path) -> Result<Loaded> {
    let text = match fs::read_to_string(path) {
        Ok(text) => Some(text),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
        Err(e) => return Err(e).with_context(|| format!("reading {}", path.display())),
    };
    let (tree, style) = match text.as_deref() {
        None => (Json::Obj(Vec::new()), Style::default()),
        Some(t) if t.trim().is_empty() => (Json::Obj(Vec::new()), Style::default()),
        Some(t) => (
            json::parse(t).with_context(|| {
                format!("{} is not strict JSON; not touching it", path.display())
            })?,
            Style::detect(t),
        ),
    };
    if !matches!(tree, Json::Obj(_)) {
        bail!(
            "{} must contain a JSON object; not touching it",
            path.display()
        );
    }
    Ok(Loaded { text, tree, style })
}

/// Follow a symlinked `settings.json` (dotfile setups) so the link survives the write.
fn write_target(settings: &Path) -> PathBuf {
    fs::canonicalize(settings).unwrap_or_else(|_| settings.to_path_buf())
}

fn backup_path_for(target: &Path) -> PathBuf {
    let mut name = target
        .file_name()
        .unwrap_or(OsStr::new("settings.json"))
        .to_owned();
    name.push(".lotse-backup");
    target.with_file_name(name)
}

/// Write a new file readable by the owner only.
fn write_private(path: &Path, bytes: &[u8]) -> Result<()> {
    let mut options = fs::OpenOptions::new();
    options.write(true).create(true).truncate(true);
    #[cfg(unix)]
    std::os::unix::fs::OpenOptionsExt::mode(&mut options, 0o600);
    let mut file = options
        .open(path)
        .with_context(|| format!("writing {}", path.display()))?;
    file.write_all(bytes)
        .with_context(|| format!("writing {}", path.display()))?;
    // `mode` only applies to files that did not exist; an old backup keeps its mode otherwise.
    #[cfg(unix)]
    fs::set_permissions(path, std::os::unix::fs::PermissionsExt::from_mode(0o600))
        .with_context(|| format!("chmod {}", path.display()))?;
    Ok(())
}

/// Temp file in the same directory, then `rename`; keeps the mode of an existing target.
fn write_atomic(target: &Path, bytes: &[u8]) -> Result<()> {
    let dir = target
        .parent()
        .context("settings path has no parent directory")?;
    let name = target.file_name().unwrap_or(OsStr::new("settings.json"));
    let mut tmp_name = std::ffi::OsString::from(".");
    tmp_name.push(name);
    tmp_name.push(format!(".lotse-tmp-{}", std::process::id()));
    let tmp = dir.join(tmp_name);
    let result = (|| -> Result<()> {
        write_private(&tmp, bytes)?;
        if let Ok(meta) = fs::metadata(target) {
            fs::set_permissions(&tmp, meta.permissions())
                .with_context(|| format!("chmod {}", tmp.display()))?;
        }
        fs::rename(&tmp, target).with_context(|| format!("replacing {}", target.display()))
    })();
    if result.is_err() {
        let _ = fs::remove_file(&tmp);
    }
    result
}

#[cfg(test)]
mod tests;
