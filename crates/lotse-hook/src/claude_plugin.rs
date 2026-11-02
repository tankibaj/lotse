//! The Lotse status plugin for Claude Code, injected per pane (no edit of `~/.claude/settings.json`).
//!
//! The daemon writes the plugin once at start and adds its directory to `CLAUDE_CODE_PLUGIN_DIRS` in
//! every pane it spawns. Claude Code loads each listed directory as an inline plugin for that process
//! only: `<dir>/.claude-plugin/plugin.json` plus the standard `<dir>/hooks/hooks.json`.
//!
//! Verified against the installed Claude Code 2.1.309 (`claude plugin validate` accepts this layout and
//! rejects an unknown hook type; the binary reads `CLAUDE_CODE_PLUGIN_DIRS` split on the OS path
//! delimiter and skips entries that are not absolute local paths). Orca uses the same env var and
//! layout for its agent-view plugin (`src/main/claude/agent-view-plugin.ts`).
//!
//! The hooks are the events of `integrations/claude/hooks.json`, the same list the global installer
//! writes. Each runs `"${LOTSE_HOOK_BIN:-lotse-hook}" claude --injected`: the daemon exports
//! `LOTSE_HOOK_BIN` into the pane, so the plugin files do not depend on where Lotse is installed. The
//! `--injected` tag lets the daemon drop the twin event when the global hooks are installed too.

use crate::claude_settings;
use anyhow::{Context, Result};
use serde_json::{Value, json};
use std::path::{Path, PathBuf};

/// Plugin name and the directory it lives in under the parent given to [`ensure`].
pub const PLUGIN_NAME: &str = "lotse-status";

/// The shell command of every hook (Claude runs it through a shell; stdin is the hook JSON).
pub const HOOK_COMMAND: &str = r#""${LOTSE_HOOK_BIN:-lotse-hook}" claude --injected"#;

/// Relative path and content of every file of the plugin.
pub fn files() -> Result<Vec<(&'static str, String)>> {
    let tpl = claude_settings::template()?;
    let mut hooks = serde_json::Map::new();
    for spec in &tpl.events {
        let mut group = serde_json::Map::new();
        if let Some(matcher) = &spec.matcher {
            group.insert("matcher".into(), json!(matcher));
        }
        group.insert(
            "hooks".into(),
            json!([{ "type": "command", "command": HOOK_COMMAND, "timeout": tpl.timeout_seconds }]),
        );
        hooks.insert(spec.event.clone(), json!([Value::Object(group)]));
    }
    let manifest = json!({
        "name": PLUGIN_NAME,
        "version": env!("CARGO_PKG_VERSION"),
        "description": "Reports this pane's Claude Code status and sub-agents to Lotse. Loaded by Lotse for its own panes only.",
        "license": "UNLICENSED",
    });
    Ok(vec![
        (".claude-plugin/plugin.json", pretty(&manifest)?),
        ("hooks/hooks.json", pretty(&json!({ "hooks": hooks }))?),
    ])
}

fn pretty(value: &Value) -> Result<String> {
    Ok(serde_json::to_string_pretty(value)? + "\n")
}

/// Write the plugin to `<parent>/lotse-status/` (files that already match are left alone, others are
/// replaced atomically so a Claude starting right now never reads half a file). Returns the plugin
/// directory: the value to put in `CLAUDE_CODE_PLUGIN_DIRS`.
pub fn ensure(parent: &Path) -> Result<PathBuf> {
    let root = parent.join(PLUGIN_NAME);
    for (relative, content) in files()? {
        let path = root.join(relative);
        if std::fs::read_to_string(&path).is_ok_and(|have| have == content) {
            continue;
        }
        let dir = path.parent().context("plugin file without a directory")?;
        std::fs::create_dir_all(dir).with_context(|| format!("creating {}", dir.display()))?;
        let tmp = dir.join(format!(".tmp-{}", std::process::id()));
        std::fs::write(&tmp, content).with_context(|| format!("writing {}", tmp.display()))?;
        std::fs::rename(&tmp, &path).with_context(|| format!("replacing {}", path.display()))?;
    }
    Ok(root)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn json_of(root: &Path, relative: &str) -> Value {
        serde_json::from_str(&std::fs::read_to_string(root.join(relative)).unwrap()).unwrap()
    }

    #[test]
    fn plugin_declares_a_hook_for_every_event_the_global_installer_covers() {
        let tmp = tempfile::tempdir().unwrap();
        let root = ensure(tmp.path()).unwrap();

        let manifest = json_of(&root, ".claude-plugin/plugin.json");
        assert_eq!(manifest["name"], PLUGIN_NAME);

        let hooks = json_of(&root, "hooks/hooks.json");
        let tpl = claude_settings::template().unwrap();
        let events = hooks["hooks"].as_object().unwrap();
        assert_eq!(events.len(), tpl.events.len());
        for spec in &tpl.events {
            let groups = events[&spec.event].as_array().unwrap();
            assert_eq!(groups.len(), 1, "{}", spec.event);
            assert_eq!(groups[0]["matcher"].as_str(), spec.matcher.as_deref());
            let handler = &groups[0]["hooks"][0];
            assert_eq!(handler["type"], "command");
            assert_eq!(handler["command"], HOOK_COMMAND);
            assert_eq!(handler["timeout"], tpl.timeout_seconds);
        }
        // the events the adapter turns into sub-agents and permission prompts
        for must in [
            "SubagentStart",
            "SubagentStop",
            "PermissionRequest",
            "StopFailure",
        ] {
            assert!(events.contains_key(must), "{must}");
        }
    }

    #[test]
    fn ensure_is_idempotent_and_repairs_a_changed_file() {
        let tmp = tempfile::tempdir().unwrap();
        let root = ensure(tmp.path()).unwrap();
        let hooks = root.join("hooks/hooks.json");
        let before = std::fs::metadata(&hooks).unwrap().modified().unwrap();
        assert_eq!(ensure(tmp.path()).unwrap(), root);
        assert_eq!(
            std::fs::metadata(&hooks).unwrap().modified().unwrap(),
            before
        );

        std::fs::write(&hooks, "{}").unwrap();
        ensure(tmp.path()).unwrap();
        assert_eq!(
            std::fs::read_to_string(&hooks).unwrap(),
            files().unwrap()[1].1
        );
        assert!(std::fs::read_dir(root.join("hooks")).unwrap().count() == 1);
    }
}
