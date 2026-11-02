//! OpenCode plugin fixtures, in two halves.
//!
//! Each file in `integrations/opencode/fixtures/` has OpenCode bus `events` and the `payloads` the
//! plugin must send to `lotse-hook` for them, with the status the engine must report after each.
//!
//! - `payloads_drive_the_adapter_to_the_expected_statuses` feeds the payloads to the adapter through
//!   the public `Engine` API (no JavaScript involved).
//! - `the_plugin_sends_exactly_the_documented_payloads` runs the real `lotse-status.js` under `node`
//!   with a fake `lotse-hook` (a shell script that appends its stdin to a file) and compares what it
//!   sent with `payloads`. It skips, loudly, when `node` is not installed.
//!
//! Tests never run OpenCode and never touch a real `lotse-hook`, socket or config directory.

use lotse_detect::opencode::OpenCodeAdapter;
use lotse_detect::{Engine, HookObservation, Observation};
use lotse_proto::{AgentKind, SessionId};
use serde_json::{Value, json};
use std::path::{Path, PathBuf};
use std::process::Command;

fn integration_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../integrations/opencode")
}

fn load_fixtures() -> Vec<(String, Value)> {
    let mut files: Vec<_> = std::fs::read_dir(integration_dir().join("fixtures"))
        .expect("fixtures dir")
        .map(|e| e.unwrap().path())
        .filter(|p| p.extension().is_some_and(|e| e == "json"))
        .collect();
    files.sort();
    assert!(files.len() >= 3, "fixtures missing");
    files
        .into_iter()
        .map(|p| {
            let name = p.file_name().unwrap().to_string_lossy().into_owned();
            let value = serde_json::from_str(&std::fs::read_to_string(&p).unwrap())
                .unwrap_or_else(|e| panic!("{name}: {e}"));
            (name, value)
        })
        .collect()
}

#[test]
fn payloads_drive_the_adapter_to_the_expected_statuses() {
    for (name, fixture) in load_fixtures() {
        assert!(
            fixture["provenance"].is_string(),
            "{name}: say where the events come from"
        );
        let mut engine = Engine::new(SessionId::new("s1"), AgentKind::OpenCode);
        engine.register(Box::new(OpenCodeAdapter::new()));
        for (i, step) in fixture["payloads"].as_array().unwrap().iter().enumerate() {
            let ctx = format!(
                "{name} payload {i} ({})",
                step["payload"]["hook_event_name"]
            );
            let update = engine.observe(Observation::Hook(HookObservation {
                agent: AgentKind::OpenCode,
                payload: step["payload"].clone(),
                at: 1000 * (i as u64 + 1),
            }));
            assert_eq!(json!(engine.status()), step["expect"], "{ctx}: status");
            if let Some(expected) = step.get("perm") {
                assert_eq!(&json!(update.and_then(|u| u.perm)), expected, "{ctx}: perm");
            }
            if let Some(expected) = step.get("sub_agents") {
                assert_eq!(&json!(engine.sub_agents()), expected, "{ctx}: sub-agents");
            }
        }
    }
}

// ---- the plugin under node ---------------------------------------------------------------------------

const DRIVER: &str = r#"
import { readFileSync } from "node:fs";
import { pathToFileURL } from "node:url";
const [, , pluginPath, eventsPath] = process.argv;
const { LotseStatusPlugin } = await import(pathToFileURL(pluginPath).href);
const hooks = await LotseStatusPlugin({});
for (const event of JSON.parse(readFileSync(eventsPath, "utf8"))) await hooks.event({ event });
"#;

const FAKE_HOOK: &str =
    "#!/bin/sh\necho \"$1\" >> \"$LOTSE_CAPTURE.args\"\n{ cat; echo; } >> \"$LOTSE_CAPTURE\"\n";

fn node_available() -> bool {
    Command::new("node")
        .arg("--version")
        .output()
        .is_ok_and(|o| o.status.success())
}

struct Run {
    /// One parsed JSON object per `lotse-hook` invocation, in arrival order.
    payloads: Vec<Value>,
    args: Vec<String>,
}

/// Run the plugin over `events` in a scratch directory. `session` is `LOTSE_SESSION_ID`, `hook` the
/// `LOTSE_HOOK_BIN` (`None`: the fake hook).
fn run_plugin(events: &Value, session: Option<&str>, hook: Option<&str>) -> Run {
    use std::os::unix::fs::PermissionsExt;
    let dir = tempfile::tempdir().unwrap();
    let work = dir.path();
    let plugin = work.join("lotse-status.mjs");
    std::fs::copy(integration_dir().join("lotse-status.js"), &plugin).unwrap();
    std::fs::write(work.join("driver.mjs"), DRIVER).unwrap();
    std::fs::write(work.join("events.json"), events.to_string()).unwrap();
    let fake = work.join("lotse-hook");
    std::fs::write(&fake, FAKE_HOOK).unwrap();
    std::fs::set_permissions(&fake, std::fs::Permissions::from_mode(0o755)).unwrap();
    let capture = work.join("capture");

    let mut cmd = Command::new("node");
    cmd.arg(work.join("driver.mjs"))
        .arg(&plugin)
        .arg(work.join("events.json"));
    cmd.env_clear()
        .env("PATH", std::env::var_os("PATH").unwrap_or_default())
        .env("HOME", work)
        .env("LOTSE_CAPTURE", &capture)
        .env(
            "LOTSE_HOOK_BIN",
            hook.map_or(fake.as_os_str(), |h| Path::new(h).as_os_str()),
        );
    if let Some(session) = session {
        cmd.env("LOTSE_SESSION_ID", session);
    }
    let out = cmd.output().expect("spawn node");
    assert!(
        out.status.success(),
        "plugin run failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );

    let lines = |path: &Path| std::fs::read_to_string(path).unwrap_or_default();
    Run {
        payloads: lines(&capture)
            .lines()
            .filter(|l| !l.is_empty())
            .map(|l| serde_json::from_str(l).unwrap_or_else(|e| panic!("bad payload {l:?}: {e}")))
            .collect(),
        args: lines(&work.join("capture.args"))
            .lines()
            .map(str::to_owned)
            .collect(),
    }
}

#[test]
fn the_plugin_sends_exactly_the_documented_payloads() {
    if !node_available() {
        eprintln!("SKIPPED: node is not installed, the plugin was not exercised");
        return;
    }
    for (name, fixture) in load_fixtures() {
        let run = run_plugin(&fixture["events"], Some("lotse-session"), None);
        let expected: Vec<&Value> = fixture["payloads"]
            .as_array()
            .unwrap()
            .iter()
            .map(|s| &s["payload"])
            .collect();
        assert_eq!(
            run.payloads.iter().collect::<Vec<_>>(),
            expected,
            "{name}: payloads, in order"
        );
        assert!(
            run.args.iter().all(|a| a == "opencode"),
            "{name}: lotse-hook is called as `lotse-hook opencode`"
        );
        assert_eq!(
            run.args.len(),
            expected.len(),
            "{name}: one hook call per payload"
        );
        // Conversation text stays in OpenCode.
        assert!(
            !json!(run.payloads).to_string().contains("secret"),
            "{name}: no conversation text"
        );
    }
}

#[test]
fn the_plugin_is_inert_outside_lotse_and_survives_a_missing_hook() {
    if !node_available() {
        eprintln!("SKIPPED: node is not installed, the plugin was not exercised");
        return;
    }
    let events = &load_fixtures()[0].1["events"];
    // No LOTSE_SESSION_ID: not a Lotse pane, nothing is sent.
    assert!(run_plugin(events, None, None).payloads.is_empty());
    // A hook binary that does not exist: OpenCode must not notice (the run still exits 0).
    assert!(
        run_plugin(
            events,
            Some("lotse-session"),
            Some("/nonexistent/lotse-hook")
        )
        .payloads
        .is_empty()
    );
}
