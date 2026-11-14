//! The environment the daemon exports on every pane is the hook's whole contract.

use lotse_hook::forward::Config;
use lotse_proto::AgentKind;
use std::collections::HashMap;
use std::ffi::OsString;
use std::path::PathBuf;

fn env(pairs: &[(&str, &str)]) -> impl Fn(&str) -> Option<OsString> {
    let map: HashMap<String, OsString> = pairs
        .iter()
        .map(|(k, v)| ((*k).to_owned(), OsString::from(v)))
        .collect();
    move |name| map.get(name).cloned()
}

#[test]
fn socket_token_file_and_session_come_from_the_daemons_variables() {
    let cfg = Config::from_env(
        AgentKind::ClaudeCode,
        env(&[
            ("LOTSE_SESSION_ID", "s-1"),
            ("LOTSE_SOCKET", "/tmp/x/lotsed.sock"),
            ("LOTSE_TOKEN_FILE", "/tmp/x/token"),
            ("LOTSE_PROJECT_ID", "p-1"),
        ]),
    )
    .unwrap();
    assert_eq!(cfg.socket, PathBuf::from("/tmp/x/lotsed.sock"));
    assert_eq!(cfg.token_file, PathBuf::from("/tmp/x/token"));
    assert_eq!(cfg.session_id.as_deref(), Some("s-1"));
}

#[test]
fn a_token_value_in_the_environment_is_ignored() {
    let cfg = Config::from_env(
        AgentKind::Codex,
        env(&[
            ("LOTSE_SESSION_ID", "s-1"),
            ("LOTSE_SOCKET", "/tmp/x/lotsed.sock"),
            ("LOTSE_TOKEN_FILE", "/tmp/x/token"),
            ("LOTSE_TOKEN", "hunter2"),
        ]),
    )
    .unwrap();
    assert_eq!(cfg.token_file, PathBuf::from("/tmp/x/token"));
}

#[test]
fn without_the_pair_the_data_dir_is_the_fallback_and_no_session_means_no_send() {
    let cfg = Config::from_env(
        AgentKind::ClaudeCode,
        env(&[
            ("LOTSE_SOCKET", "/only/socket"),
            ("LOTSE_DATA_DIR", "/data"),
        ]),
    )
    .unwrap();
    assert_eq!(cfg.socket, PathBuf::from("/data/lotsed.sock"));
    assert_eq!(cfg.token_file, PathBuf::from("/data/token"));
    assert_eq!(cfg.session_id, None, "outside a Lotse pane nothing is sent");
}
