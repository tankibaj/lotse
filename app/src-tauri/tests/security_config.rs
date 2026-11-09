//! Guards on `tauri.conf.json` and `capabilities/`: strict CSP, minimal capabilities, and the
//! packaging that puts the daemon and the hook next to the app binary.

use serde_json::Value;
use std::path::PathBuf;

fn read_json(rel: &str) -> Value {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(rel);
    let text = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    serde_json::from_str(&text).unwrap_or_else(|e| panic!("{}: {e}", path.display()))
}

fn conf() -> Value {
    read_json("tauri.conf.json")
}

fn directive<'a>(csp: &'a str, name: &str) -> Vec<&'a str> {
    csp.split(';')
        .map(str::trim)
        .find_map(|d| d.strip_prefix(name).filter(|r| r.starts_with(' ')))
        .map(|rest| rest.split_whitespace().collect())
        .unwrap_or_default()
}

fn csps() -> Vec<(&'static str, String)> {
    let conf = conf();
    ["csp", "devCsp"]
        .into_iter()
        .map(|k| {
            let csp = conf["app"]["security"][k].as_str().unwrap_or_default();
            assert!(!csp.is_empty(), "app.security.{k} must be set");
            (k, csp.to_owned())
        })
        .collect()
}

#[test]
fn csp_has_no_eval_and_no_remote_origins() {
    for (name, csp) in csps() {
        assert!(!csp.contains("unsafe-eval"), "{name}: unsafe-eval");
        assert!(
            !csp.contains("wasm-unsafe-eval"),
            "{name}: wasm-unsafe-eval"
        );
        for source in csp.split([';', ' ']).filter(|s| !s.is_empty()) {
            let remote = source.contains("://") || source.starts_with("//") || source == "*";
            let allowed = matches!(
                source,
                // Tauri's IPC on macOS/Windows, and the dev server's hot-reload socket
                "http://ipc.localhost" | "ws://localhost:1420"
            );
            assert!(!remote || allowed, "{name}: remote source {source:?}");
        }
        assert_eq!(directive(&csp, "default-src"), ["'self'"], "{name}");
        for closed in ["object-src", "base-uri", "form-action"] {
            assert_eq!(directive(&csp, closed), ["'none'"], "{name}: {closed}");
        }
    }
}

#[test]
fn release_scripts_run_from_self_only() {
    let conf = conf();
    let csp = conf["app"]["security"]["csp"].as_str().unwrap();
    assert_eq!(directive(csp, "script-src"), ["'self'"]);
    assert_eq!(
        directive(csp, "connect-src"),
        ["ipc:", "http://ipc.localhost"]
    );
}

#[test]
fn capabilities_are_explicit_and_minimal() {
    let cap = read_json("capabilities/default.json");
    assert_eq!(cap["windows"], serde_json::json!(["main"]));
    let perms: Vec<&str> = cap["permissions"]
        .as_array()
        .expect("permissions")
        .iter()
        .map(|p| {
            p.as_str()
                .expect("plain permission identifiers, no scoped objects")
        })
        .collect();
    // what the webview calls: daemon events (listen/unlisten, also drag-drop), the title-bar drag
    // region, the folder picker, and the transcript export's save dialog. A missing one fails silently at runtime.
    for needed in [
        "core:event:allow-listen",
        "core:event:allow-unlisten",
        "core:window:allow-start-dragging",
        "dialog:allow-open",
        "dialog:allow-save",
    ] {
        assert!(perms.contains(&needed), "missing {needed}");
    }
    for p in &perms {
        assert!(!p.contains('*'), "wildcard permission {p}");
        // default sets grant more than the UI calls (path, image, menu, tray, event emit, …)
        assert!(
            !p.ends_with(":default"),
            "default set {p}: list the permissions instead"
        );
        let plugin = p.split(':').next().unwrap();
        assert!(
            matches!(plugin, "core" | "dialog"),
            "{p}: a new plugin needs review (fs, shell, http, opener, … give the webview reach)"
        );
    }
    assert!(
        !perms
            .iter()
            .any(|p| p.starts_with("core:webview:allow-internal-toggle-devtools"))
    );
}

#[test]
fn capability_set_is_exact() {
    let cap = read_json("capabilities/default.json");
    let mut perms: Vec<&str> = cap["permissions"]
        .as_array()
        .expect("permissions")
        .iter()
        .map(|p| p.as_str().expect("plain permission identifiers"))
        .collect();
    perms.sort_unstable();
    // a new permission is a decision: it changes this list in the same diff
    assert_eq!(
        perms,
        [
            "core:event:allow-listen",
            "core:event:allow-unlisten",
            "core:window:allow-internal-toggle-maximize",
            "core:window:allow-start-dragging",
            "dialog:allow-open",
            "dialog:allow-save",
        ]
    );
}

#[test]
fn bundle_ships_the_daemon_and_the_hook_next_to_the_app_binary() {
    let conf = conf();
    let files = &conf["bundle"]["macOS"]["files"];
    assert_eq!(files["MacOS/lotsed"], "../../target/release/lotsed");
    assert_eq!(files["MacOS/lotse-hook"], "../../target/release/lotse-hook");
    assert!(
        conf["app"]["security"]["dangerousDisableAssetCspModification"].is_null(),
        "CSP modification must stay on"
    );
}
