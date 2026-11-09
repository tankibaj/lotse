//! "Manage" in the session panel's connector list: opens an agent config file in the default text editor
//! (`open -t`). Only a path the daemon listed as a connector's `config_path` opens: the webview names
//! a session and a path, and the daemon's own answer decides.

use crate::daemon::Bridge;
use lotse_proto::{Connector, Request, Response, SessionId};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::Arc;

/// The file to open: `path`, only when one of `connectors` declares it verbatim and it is a file.
pub fn listed_config(connectors: &[Connector], path: &str) -> Result<PathBuf, String> {
    if !connectors.iter().any(|c| c.config_path == path) {
        return Err(format!("{path} is not a config file the daemon listed"));
    }
    let file = Path::new(path);
    if !file.is_absolute() || !file.is_file() {
        return Err(format!("{path} is not a file"));
    }
    Ok(file.to_path_buf())
}

/// Open the config file `path` of `session_id`'s connectors in the default text editor.
#[tauri::command]
pub async fn open_agent_config(
    bridge: tauri::State<'_, Arc<Bridge>>,
    session_id: String,
    path: String,
) -> Result<(), String> {
    let request = Request::ListConnectors {
        session_id: SessionId::new(session_id),
    };
    let Response::Connectors(connectors) = bridge.request(request).await? else {
        return Err("the daemon did not list connectors".into());
    };
    let file = listed_config(&connectors, &path)?;
    let mut child = Command::new("/usr/bin/open")
        .arg("-t")
        .arg(&file)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| format!("open: {e}"))?;
    std::thread::spawn(move || {
        let _ = child.wait(); // reap, no zombie
    });
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use lotse_proto::{AgentKind, ConnectorStatus};

    fn connector(path: &str) -> Connector {
        Connector {
            agent: AgentKind::ClaudeCode,
            name: "github".into(),
            transport: "stdio".into(),
            config_path: path.into(),
            status: ConnectorStatus::Configured,
        }
    }

    #[test]
    fn config_open_refuses_unknown_path() {
        let dir = tempfile::tempdir().unwrap();
        let listed = dir.path().join(".mcp.json");
        let other = dir.path().join("secrets.txt");
        std::fs::write(&listed, "{}").unwrap();
        std::fs::write(&other, "x").unwrap();
        let connectors = [connector(listed.to_str().unwrap())];
        // a real file, but the daemon did not list it
        assert!(listed_config(&connectors, other.to_str().unwrap()).is_err());
        // `..` and relative forms of the listed path do not match either
        let dotted = format!("{}/../{}", dir.path().display(), ".mcp.json");
        assert!(listed_config(&connectors, &dotted).is_err());
        assert!(listed_config(&connectors, ".mcp.json").is_err());
        assert!(listed_config(&[], listed.to_str().unwrap()).is_err());
        assert!(listed_config(&connectors, "").is_err());
    }

    #[test]
    fn config_open_accepts_a_listed_file() {
        let dir = tempfile::tempdir().unwrap();
        let listed = dir.path().join(".mcp.json");
        std::fs::write(&listed, "{}").unwrap();
        let connectors = [connector("/nowhere"), connector(listed.to_str().unwrap())];
        assert_eq!(
            listed_config(&connectors, listed.to_str().unwrap()),
            Ok(listed)
        );
    }

    #[test]
    fn config_open_refuses_a_listed_path_that_is_not_a_file() {
        let dir = tempfile::tempdir().unwrap();
        let missing = dir.path().join("gone.json");
        let connectors = [
            connector(missing.to_str().unwrap()),
            connector(dir.path().to_str().unwrap()),
        ];
        assert!(listed_config(&connectors, missing.to_str().unwrap()).is_err());
        assert!(listed_config(&connectors, dir.path().to_str().unwrap()).is_err());
    }
}
