//! Installer tests. Every test works in a temp directory standing in for `CLAUDE_CONFIG_DIR`;
//! nothing reads the environment or touches the real `~/.claude`.

use super::*;
use serde_json::Value;
use tempfile::TempDir;

const HOOK: &str = "/Applications/Lotse.app/Contents/MacOS/lotse-hook";

fn dir() -> TempDir {
    tempfile::tempdir().unwrap()
}

fn hook() -> PathBuf {
    PathBuf::from(HOOK)
}

fn settings(d: &TempDir) -> PathBuf {
    settings_path(d.path())
}

fn put(d: &TempDir, text: &str) {
    fs::write(settings(d), text).unwrap();
}

fn read(d: &TempDir) -> String {
    fs::read_to_string(settings(d)).unwrap()
}

fn parsed(d: &TempDir) -> Value {
    serde_json::from_str(&read(d)).unwrap()
}

/// What Claude Code writes: `JSON.stringify(settings, null, 2)` plus a newline.
const USER_SETTINGS: &str = r#"{
  "model": "opus",
  "env": {
    "FOO": "bar \u00e9 \"quoted\""
  },
  "permissions": {
    "allow": [
      "Bash(ls:*)",
      "Read"
    ],
    "deny": []
  },
  "hooks": {
    "PreToolUse": [
      {
        "matcher": "Bash",
        "hooks": [
          {
            "type": "command",
            "command": "/usr/local/bin/audit-bash"
          }
        ]
      }
    ],
    "Stop": [
      {
        "hooks": [
          {
            "type": "command",
            "command": "say done",
            "timeout": 10
          }
        ]
      }
    ]
  },
  "statusLine": {
    "type": "command",
    "command": "~/.claude/statusline.sh"
  }
}
"#;

fn events_with_marker(v: &Value) -> Vec<String> {
    let mut found: Vec<String> = v["hooks"]
        .as_object()
        .into_iter()
        .flatten()
        .filter(|(_, groups)| groups.to_string().contains("lotse:managed"))
        .map(|(event, _)| event.clone())
        .collect();
    found.sort();
    found
}

#[test]
fn template_is_valid_and_lists_the_events_the_adapter_needs() {
    let tpl = template().unwrap();
    assert_eq!(tpl.marker, "# lotse:managed");
    assert!(tpl.timeout_seconds > 0 && tpl.timeout_seconds <= 10);
    let events: Vec<&str> = tpl.events.iter().map(|e| e.event.as_str()).collect();
    for needed in [
        "SessionStart",
        "SessionEnd",
        "UserPromptSubmit",
        "PreToolUse",
        "PostToolUse",
        "PostToolUseFailure",
        "PermissionRequest",
        "Stop",
        "StopFailure",
        "SubagentStart",
        "SubagentStop",
    ] {
        assert!(events.contains(&needed), "template lacks {needed}");
    }
    let mut unique = events.clone();
    unique.sort();
    unique.dedup();
    assert_eq!(
        unique.len(),
        events.len(),
        "duplicate events in the template"
    );
}

#[test]
fn hook_command_quotes_the_path_and_ends_in_the_marker() {
    let cmd = hook_command(
        Path::new("/Users/o'brien/Lotse App/lotse-hook"),
        "# lotse:managed",
    )
    .unwrap();
    assert_eq!(
        cmd,
        "'/Users/o'\\''brien/Lotse App/lotse-hook' claude # lotse:managed"
    );
}

#[test]
fn config_dir_prefers_claude_config_dir() {
    let home = Path::new("/h");
    assert_eq!(
        config_dir(Some(OsStr::new("/cfg")), Some(home)),
        Some(PathBuf::from("/cfg"))
    );
    assert_eq!(
        config_dir(None, Some(home)),
        Some(PathBuf::from("/h/.claude"))
    );
    assert_eq!(
        config_dir(Some(OsStr::new("")), Some(home)),
        Some(PathBuf::from("/h/.claude"))
    );
    assert_eq!(config_dir(None, None), None);
}

#[test]
fn install_creates_settings_with_every_event() {
    let d = dir();
    let out = install(d.path(), &hook()).unwrap();
    assert_eq!(out, InstallOutcome::Written { backup: None });
    let v = parsed(&d);
    let tpl = template().unwrap();
    for spec in &tpl.events {
        let groups = v["hooks"][&spec.event].as_array().unwrap();
        assert_eq!(groups.len(), 1, "{}", spec.event);
        let group = &groups[0];
        assert_eq!(
            group.get("matcher").and_then(Value::as_str),
            spec.matcher.as_deref()
        );
        let handler = &group["hooks"][0];
        assert_eq!(handler["type"], "command");
        assert_eq!(handler["timeout"], tpl.timeout_seconds);
        let command = handler["command"].as_str().unwrap();
        assert!(
            command.starts_with(&format!("'{HOOK}' claude ")),
            "{command}"
        );
        assert!(command.ends_with("# lotse:managed"));
    }
    assert!(read(&d).ends_with("}\n"));
    assert_eq!(status(d.path()).unwrap(), InstallStatus::Installed);
}

#[test]
fn install_is_idempotent_and_does_not_rewrite() {
    let d = dir();
    put(&d, USER_SETTINGS);
    install(d.path(), &hook()).unwrap();
    let first = read(&d);
    let mtime = fs::metadata(settings(&d)).unwrap().modified().unwrap();
    assert_eq!(
        install(d.path(), &hook()).unwrap(),
        InstallOutcome::Unchanged
    );
    assert_eq!(read(&d), first);
    assert_eq!(
        fs::metadata(settings(&d)).unwrap().modified().unwrap(),
        mtime
    );
    // Still exactly one Lotse handler per event.
    let v = parsed(&d);
    for event in events_with_marker(&v) {
        assert_eq!(
            v["hooks"][&event]
                .to_string()
                .matches("lotse:managed")
                .count(),
            1,
            "{event}"
        );
    }
}

#[test]
fn round_trip_is_byte_identical_to_the_backup() {
    let d = dir();
    put(&d, USER_SETTINGS);
    let out = install(d.path(), &hook()).unwrap();
    let backup = backup_path(d.path());
    assert_eq!(
        out,
        InstallOutcome::Written {
            backup: Some(backup.clone())
        }
    );
    assert_eq!(fs::read_to_string(&backup).unwrap(), USER_SETTINGS);
    assert_ne!(read(&d), USER_SETTINGS);

    assert_eq!(
        uninstall(d.path()).unwrap(),
        UninstallOutcome::RestoredBackup
    );
    assert_eq!(fs::read(settings(&d)).unwrap(), fs::read(&backup).unwrap());
    assert_eq!(read(&d), USER_SETTINGS);
    assert_eq!(status(d.path()).unwrap(), InstallStatus::NotInstalled);
    assert_eq!(uninstall(d.path()).unwrap(), UninstallOutcome::NotInstalled);
}

#[test]
fn round_trip_is_byte_identical_for_unusual_formatting() {
    let originals = [
        // Compact one-liner with a trailing newline.
        "{\"model\":\"opus\",\"hooks\":{\"Stop\":[{\"hooks\":[{\"type\":\"command\",\"command\":\"x\"}]}]}}\n",
        // Tabs, CRLF, no final newline, escaped key, odd number formats, empty hooks object.
        "{\r\n\t\"\\u006dodel\": \"opus\",\r\n\t\"n\": 1.50E+2,\r\n\t\"hooks\": {}\r\n}",
        // No hooks key at all, four-space indent.
        "{\n    \"theme\": \"dark\"\n}\n",
        // Pre-existing empty event array.
        "{\n  \"hooks\": {\n    \"Stop\": []\n  }\n}\n",
        // Just an empty object.
        "{}",
        // Empty file.
        "",
    ];
    for original in originals {
        let d = dir();
        put(&d, original);
        install(d.path(), &hook()).unwrap();
        assert!(
            status(d.path()).unwrap() == InstallStatus::Installed,
            "{original:?}"
        );
        uninstall(d.path()).unwrap();
        assert_eq!(read(&d), original, "round trip changed {original:?}");
    }
}

#[test]
fn install_keeps_everything_else_and_user_hooks() {
    let d = dir();
    put(&d, USER_SETTINGS);
    install(d.path(), &hook()).unwrap();
    let v = parsed(&d);
    let before: Value = serde_json::from_str(USER_SETTINGS).unwrap();
    for key in ["model", "env", "permissions", "statusLine"] {
        assert_eq!(v[key], before[key], "{key}");
    }
    // User hooks stay first, ours is appended as its own group.
    let pre = v["hooks"]["PreToolUse"].as_array().unwrap();
    assert_eq!(pre.len(), 2);
    assert_eq!(pre[0], before["hooks"]["PreToolUse"][0]);
    let stop = v["hooks"]["Stop"].as_array().unwrap();
    assert_eq!(stop[0], before["hooks"]["Stop"][0]);
    // Key order of the file is kept: model, env, permissions, hooks, statusLine.
    let text = read(&d);
    let at = |k: &str| text.find(&format!("\n  \"{k}\"")).unwrap();
    assert!(at("model") < at("env") && at("env") < at("permissions"));
    assert!(at("permissions") < at("hooks") && at("hooks") < at("statusLine"));
    // Unrelated string escapes survive untouched.
    assert!(text.contains(r#""FOO": "bar \u00e9 \"quoted\"""#));
}

#[test]
fn uninstall_after_user_edits_keeps_the_edits() {
    let d = dir();
    put(&d, USER_SETTINGS);
    install(d.path(), &hook()).unwrap();
    // The user changes a setting and adds a hook of their own while Lotse is installed.
    let edited = read(&d)
        .replace("\"model\": \"opus\"", "\"model\": \"sonnet\"")
        .replace("\"deny\": []", "\"deny\": [\n      \"Bash(rm:*)\"\n    ]");
    put(&d, &edited);

    assert_eq!(uninstall(d.path()).unwrap(), UninstallOutcome::Removed);
    let v = parsed(&d);
    assert_eq!(v["model"], "sonnet");
    assert_eq!(v["permissions"]["deny"][0], "Bash(rm:*)");
    assert!(events_with_marker(&v).is_empty());
    // User hooks are still there; events only Lotse used are gone.
    assert_eq!(v["hooks"]["PreToolUse"].as_array().unwrap().len(), 1);
    assert_eq!(v["hooks"]["Stop"].as_array().unwrap().len(), 1);
    assert_eq!(v["hooks"].as_object().unwrap().len(), 2);
    // Output is in Claude's format.
    assert!(read(&d).ends_with("}\n"));
}

#[test]
fn uninstall_removes_a_marked_handler_from_inside_a_user_group() {
    let d = dir();
    put(
        &d,
        r##"{
  "hooks": {
    "Stop": [
      {
        "hooks": [
          { "type": "command", "command": "say done" },
          { "type": "command", "command": "'/x/lotse-hook' claude # lotse:managed", "timeout": 5 }
        ]
      }
    ]
  }
}
"##,
    );
    assert!(matches!(
        status(d.path()).unwrap(),
        InstallStatus::Partial { .. }
    ));
    assert_eq!(uninstall(d.path()).unwrap(), UninstallOutcome::Removed);
    let v = parsed(&d);
    let handlers = v["hooks"]["Stop"][0]["hooks"].as_array().unwrap();
    assert_eq!(handlers.len(), 1);
    assert_eq!(handlers[0]["command"], "say done");
}

#[test]
fn reinstall_with_a_moved_binary_updates_in_place_and_keeps_the_first_backup() {
    let d = dir();
    put(&d, USER_SETTINGS);
    install(d.path(), &hook()).unwrap();
    let moved = PathBuf::from("/opt/lotse/bin/lotse-hook");
    assert_eq!(
        install(d.path(), &moved).unwrap(),
        InstallOutcome::Written { backup: None },
        "an upgrade must not replace the pre-Lotse backup"
    );
    let text = read(&d);
    assert!(!text.contains(HOOK));
    assert_eq!(
        text.matches("'/opt/lotse/bin/lotse-hook' claude").count(),
        template().unwrap().events.len()
    );
    assert_eq!(
        fs::read_to_string(backup_path(d.path())).unwrap(),
        USER_SETTINGS
    );
    // And the round trip still lands on the original.
    uninstall(d.path()).unwrap();
    assert_eq!(read(&d), USER_SETTINGS);
}

#[test]
fn a_fresh_install_after_a_manual_uninstall_refreshes_the_backup() {
    let d = dir();
    put(&d, USER_SETTINGS);
    install(d.path(), &hook()).unwrap();
    uninstall(d.path()).unwrap();
    let newer = USER_SETTINGS.replace("opus", "haiku");
    put(&d, &newer);
    install(d.path(), &hook()).unwrap();
    assert_eq!(fs::read_to_string(backup_path(d.path())).unwrap(), newer);
}

#[test]
fn unparsable_or_oddly_shaped_settings_are_left_untouched() {
    let cases = [
        "{ // comment\n}",
        "{\"hooks\":{},}",
        "[]",
        "\"x\"",
        "{\"hooks\": []}",
        "{\"hooks\": {\"Stop\": {}}}",
        "{\"hooks\": {\"PreToolUse\": \"nope\"}}",
        "{",
    ];
    for original in cases {
        let d = dir();
        put(&d, original);
        let err = install(d.path(), &hook()).unwrap_err().to_string();
        assert!(!err.is_empty());
        assert_eq!(read(&d), original, "install touched {original:?}");
        assert!(!backup_path(d.path()).exists(), "backup for {original:?}");
        assert_eq!(
            fs::read_dir(d.path()).unwrap().count(),
            1,
            "stray files for {original:?}"
        );
    }
}

#[test]
fn uninstall_and_status_without_settings() {
    let d = dir();
    assert_eq!(uninstall(d.path()).unwrap(), UninstallOutcome::NotInstalled);
    assert_eq!(status(d.path()).unwrap(), InstallStatus::NotInstalled);
    assert!(!settings(&d).exists(), "uninstall must not create the file");
    // A config dir that does not exist yet is created by install.
    let nested = d.path().join("new").join("claude");
    install(&nested, &hook()).unwrap();
    assert!(settings_path(&nested).exists());
}

#[test]
fn status_reports_missing_events() {
    let d = dir();
    install(d.path(), &hook()).unwrap();
    let text = read(&d);
    // Break the Stop hook: it no longer carries the marker.
    let edited = text.replacen("claude # lotse:managed", "claude", 1);
    assert_ne!(edited, text);
    put(&d, &edited);
    match status(d.path()).unwrap() {
        InstallStatus::Partial { missing } => assert_eq!(missing.len(), 1),
        other => panic!("{other:?}"),
    }
}

#[cfg(unix)]
#[test]
fn modes_are_kept_and_the_backup_is_private() {
    use std::os::unix::fs::PermissionsExt;
    let d = dir();
    put(&d, USER_SETTINGS);
    fs::set_permissions(settings(&d), fs::Permissions::from_mode(0o640)).unwrap();
    install(d.path(), &hook()).unwrap();
    let mode = |p: &Path| fs::metadata(p).unwrap().permissions().mode() & 0o777;
    assert_eq!(mode(&settings(&d)), 0o640);
    assert_eq!(mode(&backup_path(d.path())), 0o600);
    uninstall(d.path()).unwrap();
    assert_eq!(mode(&settings(&d)), 0o640);
}

#[cfg(unix)]
#[test]
fn a_symlinked_settings_file_is_written_through() {
    let d = dir();
    let real = d.path().join("dotfiles");
    fs::create_dir(&real).unwrap();
    fs::write(real.join("claude-settings.json"), USER_SETTINGS).unwrap();
    let cfg = d.path().join("claude");
    fs::create_dir(&cfg).unwrap();
    std::os::unix::fs::symlink(real.join("claude-settings.json"), cfg.join("settings.json"))
        .unwrap();

    install(&cfg, &hook()).unwrap();
    assert!(
        fs::symlink_metadata(cfg.join("settings.json"))
            .unwrap()
            .file_type()
            .is_symlink()
    );
    assert!(
        fs::read_to_string(real.join("claude-settings.json"))
            .unwrap()
            .contains("lotse:managed")
    );
    uninstall(&cfg).unwrap();
    assert!(
        fs::symlink_metadata(cfg.join("settings.json"))
            .unwrap()
            .file_type()
            .is_symlink()
    );
    assert_eq!(
        fs::read_to_string(real.join("claude-settings.json")).unwrap(),
        USER_SETTINGS
    );
}
