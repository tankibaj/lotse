//! Runs the OpenCode plugin source under Node (OpenCode itself is never started): what it sends to
//! `lotse-hook` for a status event, and that two copies of it in one process (the global install
//! and the per-pane injection) report once. Skipped, with a note on stderr, when `node` is missing.

use lotse_hook::opencode_plugin::PLUGIN_SOURCE;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::process::Command;

fn node() -> Option<std::path::PathBuf> {
    std::env::var_os("PATH").and_then(|paths| {
        std::env::split_paths(&paths)
            .map(|d| d.join("node"))
            .find(|p| p.is_file())
    })
}

/// Load `copies` plugin files (each a distinct path), build each for every directory in `dirs`, send
/// one `session.status` busy event to every instance; returns the payloads `lotse-hook` received.
fn run(copies: usize, dirs: &[&str]) -> Option<Vec<serde_json::Value>> {
    let Some(node) = node() else {
        eprintln!("skipped: node is not installed");
        return None;
    };
    let tmp = tempfile::tempdir().unwrap();
    let out = tmp.path().join("received.jsonl");
    let fake = tmp.path().join("lotse-hook");
    std::fs::write(
        &fake,
        "#!/bin/sh\npayload=$(cat)\nprintf '%s\\n' \"$payload\" >> \"$OUT\"\n",
    )
    .unwrap();
    std::fs::set_permissions(&fake, std::fs::Permissions::from_mode(0o755)).unwrap();

    let mut driver = String::new();
    for i in 0..copies {
        std::fs::write(tmp.path().join(format!("plugin{i}.mjs")), PLUGIN_SOURCE).unwrap();
        driver += &format!("import {{ LotseStatusPlugin as P{i} }} from './plugin{i}.mjs';\n");
    }
    driver += "const hooks = [];\n";
    for i in 0..copies {
        for dir in dirs {
            driver += &format!("hooks.push(await P{i}({{ directory: '{dir}' }}));\n");
        }
    }
    driver += "for (const h of hooks) await h.event?.({ event: { type: 'session.status', \
               properties: { sessionID: 'ses_1', status: { type: 'busy' } } } });\n";
    std::fs::write(tmp.path().join("driver.mjs"), driver).unwrap();

    let status = Command::new(node)
        .arg("driver.mjs")
        .current_dir(tmp.path())
        .env_clear()
        .env("LOTSE_SESSION_ID", "s1")
        .env("LOTSE_HOOK_BIN", &fake)
        .env("OUT", &out)
        .env("PATH", "/usr/bin:/bin")
        .status()
        .unwrap();
    assert!(status.success());
    let text = std::fs::read_to_string(&out).unwrap_or_default();
    Some(
        text.lines()
            .filter(|l| !l.trim().is_empty())
            .map(|l| serde_json::from_str(l).unwrap())
            .collect(),
    )
}

#[test]
fn a_status_event_reaches_lotse_hook_as_the_adapter_expects() {
    let Some(got) = run(1, &["/p"]) else { return };
    assert_eq!(
        got,
        [serde_json::json!({
            "hook_event_name": "SessionStatus",
            "session_id": "ses_1",
            "status": "busy",
        })]
    );
}

#[test]
fn two_copies_of_the_plugin_in_one_process_report_once_per_directory() {
    let Some(got) = run(2, &["/p"]) else { return };
    assert_eq!(got.len(), 1, "{got:?}");
    // a second project directory in the same process is still reported (by the first copy)
    let Some(got) = run(2, &["/p", "/q"]) else {
        return;
    };
    assert_eq!(got.len(), 2, "{got:?}");
}

#[test]
fn the_plugin_source_is_valid_javascript_for_node() {
    let Some(node) = node() else { return };
    let tmp = tempfile::tempdir().unwrap();
    let file: &Path = &tmp.path().join("p.mjs");
    std::fs::write(file, PLUGIN_SOURCE).unwrap();
    let ok = Command::new(node)
        .arg("--check")
        .arg(file)
        .status()
        .unwrap();
    assert!(ok.success());
}
