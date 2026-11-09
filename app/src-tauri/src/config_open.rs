//! "Manage" in the session panel's connector list: opens an agent config file in the default text editor
//! (`open -t`). Only a path the daemon listed as a connector's `config_path` opens: the webview names
//! a session and a path, and the daemon's own answer decides. The resolved file (symlinks followed)
//! must also stay inside the user's home or the session's directory.

use crate::daemon::Bridge;
use lotse_proto::{Connector, Request, Response, SessionId};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::Arc;

/// The file to open: `path`, only when one of `connectors` declares it verbatim, it is a file, and its
/// canonical target (symlinks resolved) lies inside one of `roots`. Returns the canonical path.
pub fn listed_config(
    connectors: &[Connector],
    path: &str,
    roots: &[PathBuf],
) -> Result<PathBuf, String> {
    if !connectors.iter().any(|c| c.config_path == path) {
        return Err(format!("{path} is not a config file the daemon listed"));
    }
    let file = Path::new(path);
    if !file.is_absolute() {
        return Err(format!("{path} is not a file"));
    }
    let resolved = file
        .canonicalize()
        .map_err(|_| format!("{path} is not a file"))?;
    if !resolved.is_file() {
        return Err(format!("{path} is not a file"));
    }
    let inside = roots
        .iter()
        .filter_map(|r| r.canonicalize().ok())
        .any(|r| resolved.starts_with(r));
    if !inside {
        return Err(format!(
            "{path} resolves outside the home and project directories"
        ));
    }
    Ok(resolved)
}

/// Open the config file `path` of `session_id`'s connectors in the default text editor.
#[tauri::command]
pub async fn open_agent_config(
    bridge: tauri::State<'_, Arc<Bridge>>,
    session_id: String,
    path: String,
) -> Result<(), String> {
    let session_id = SessionId::new(session_id);
    let request = Request::ListConnectors {
        session_id: session_id.clone(),
    };
    let Response::Connectors(connectors) = bridge.request(request).await? else {
        return Err("the daemon did not list connectors".into());
    };
    let request = Request::GetSessionInfo { session_id };
    let Response::SessionInfo(info) = bridge.request(request).await? else {
        return Err("the daemon did not return the session".into());
    };
    let mut roots: Vec<PathBuf> = dirs::home_dir().into_iter().collect();
    roots.push(PathBuf::from(info.cwd));
    roots.extend(info.worktree.map(PathBuf::from));
    // resolved here, right before `open`: what is opened is the canonical file just checked
    let file = listed_config(&connectors, &path, &roots)?;
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

    /// A temp dir with its canonical path (macOS `/var` is a symlink to `/private/var`).
    fn root() -> (tempfile::TempDir, PathBuf) {
        let dir = tempfile::tempdir().unwrap();
        let canonical = dir.path().canonicalize().unwrap();
        (dir, canonical)
    }

    #[test]
    fn config_open_refuses_unknown_path() {
        let (_dir, dir) = root();
        let listed = dir.join(".mcp.json");
        let other = dir.join("secrets.txt");
        std::fs::write(&listed, "{}").unwrap();
        std::fs::write(&other, "x").unwrap();
        let roots = [dir.clone()];
        let connectors = [connector(listed.to_str().unwrap())];
        // a real file, but the daemon did not list it
        assert!(listed_config(&connectors, other.to_str().unwrap(), &roots).is_err());
        // `..` and relative forms of the listed path do not match either
        let dotted = format!("{}/../{}", dir.display(), ".mcp.json");
        assert!(listed_config(&connectors, &dotted, &roots).is_err());
        assert!(listed_config(&connectors, ".mcp.json", &roots).is_err());
        assert!(listed_config(&[], listed.to_str().unwrap(), &roots).is_err());
        assert!(listed_config(&connectors, "", &roots).is_err());
    }

    #[test]
    fn config_open_accepts_a_listed_file() {
        let (_dir, dir) = root();
        let listed = dir.join(".mcp.json");
        std::fs::write(&listed, "{}").unwrap();
        let connectors = [connector("/nowhere"), connector(listed.to_str().unwrap())];
        assert_eq!(
            listed_config(&connectors, listed.to_str().unwrap(), &[dir]),
            Ok(listed)
        );
    }

    #[test]
    fn config_open_refuses_a_listed_path_that_is_not_a_file() {
        let (_dir, dir) = root();
        let missing = dir.join("gone.json");
        let connectors = [
            connector(missing.to_str().unwrap()),
            connector(dir.to_str().unwrap()),
        ];
        let roots = [dir.clone()];
        assert!(listed_config(&connectors, missing.to_str().unwrap(), &roots).is_err());
        assert!(listed_config(&connectors, dir.to_str().unwrap(), &roots).is_err());
    }

    #[test]
    fn config_open_refuses_a_symlink_that_escapes_the_root() {
        let (_home, home) = root();
        let (_outside, outside) = root();
        let secret = outside.join("id_rsa");
        std::fs::write(&secret, "x").unwrap();
        let link = home.join(".mcp.json");
        std::os::unix::fs::symlink(&secret, &link).unwrap();
        let connectors = [connector(link.to_str().unwrap())];
        assert!(listed_config(&connectors, link.to_str().unwrap(), &[home]).is_err());
    }

    #[test]
    fn config_open_follows_a_symlink_that_stays_inside_the_root() {
        let (_home, home) = root();
        let real = home.join("real.json");
        std::fs::write(&real, "{}").unwrap();
        let link = home.join(".mcp.json");
        std::os::unix::fs::symlink(&real, &link).unwrap();
        let connectors = [connector(link.to_str().unwrap())];
        // opens the resolved target, not the link
        assert_eq!(
            listed_config(&connectors, link.to_str().unwrap(), &[home]),
            Ok(real)
        );
    }

    #[test]
    fn config_open_refuses_a_file_outside_every_root() {
        let (_home, home) = root();
        let (_other, other) = root();
        let file = other.join("settings.json");
        std::fs::write(&file, "{}").unwrap();
        let connectors = [connector(file.to_str().unwrap())];
        assert!(listed_config(&connectors, file.to_str().unwrap(), &[home]).is_err());
        assert!(listed_config(&connectors, file.to_str().unwrap(), &[]).is_err());
    }
}
