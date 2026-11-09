//! "Manage" in the session panel's connector list: opens an agent config file in the default text editor
//! (`open -t`). Only a path the daemon listed as a connector's `config_path` opens: the webview names
//! a session and a path, and the daemon's own answer decides. The resolved file (symlinks followed)
//! must also stay inside the user's home or the session's directory.

use crate::daemon::Bridge;
use lotse_proto::{Connector, Request, Response, SessionId};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::Arc;

/// The file to open: `path`, only when one of `connectors` declares it verbatim and it is a file.
/// Its canonical target (symlinks resolved) must stay inside the root the path sits in: a path under
/// a session directory (`project_roots`: cwd, worktree; a repo controls what is in there) resolves
/// inside that same directory, any other (user-level) path inside `home`. A root of `/` is no root.
/// Returns the canonical path.
pub fn listed_config(
    connectors: &[Connector],
    path: &str,
    home: Option<&Path>,
    project_roots: &[PathBuf],
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
    // where the path itself sits: its directory resolved, the final component left alone
    let location = match (file.parent().map(Path::canonicalize), file.file_name()) {
        (Some(Ok(dir)), Some(name)) => dir.join(name),
        _ => return Err(format!("{path} is not a file")),
    };
    let canonical = |r: &PathBuf| r.canonicalize().ok().filter(|r| r.parent().is_some());
    let root = match project_roots
        .iter()
        .filter_map(canonical)
        .find(|r| location.starts_with(r))
    {
        Some(project) => Some(project),
        None => home.and_then(|h| canonical(&h.to_path_buf())),
    };
    if !root.is_some_and(|r| resolved.starts_with(r)) {
        return Err(format!(
            "{path} resolves outside the directory it is configured in"
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
    let mut project_roots = vec![PathBuf::from(info.cwd)];
    project_roots.extend(info.worktree.map(PathBuf::from));
    // resolved here, right before `open`: what is opened is the canonical file just checked
    let file = listed_config(
        &connectors,
        &path,
        dirs::home_dir().as_deref(),
        &project_roots,
    )?;
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
        let home = Some(dir.as_path());
        let connectors = [connector(listed.to_str().unwrap())];
        // a real file, but the daemon did not list it
        assert!(listed_config(&connectors, other.to_str().unwrap(), home, &[]).is_err());
        // `..` and relative forms of the listed path do not match either
        let dotted = format!("{}/../{}", dir.display(), ".mcp.json");
        assert!(listed_config(&connectors, &dotted, home, &[]).is_err());
        assert!(listed_config(&connectors, ".mcp.json", home, &[]).is_err());
        assert!(listed_config(&[], listed.to_str().unwrap(), home, &[]).is_err());
        assert!(listed_config(&connectors, "", home, &[]).is_err());
    }

    #[test]
    fn config_open_accepts_a_listed_file() {
        let (_dir, dir) = root();
        let listed = dir.join(".mcp.json");
        std::fs::write(&listed, "{}").unwrap();
        let connectors = [connector("/nowhere"), connector(listed.to_str().unwrap())];
        assert_eq!(
            listed_config(&connectors, listed.to_str().unwrap(), Some(&dir), &[]),
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
        let home = Some(dir.as_path());
        assert!(listed_config(&connectors, missing.to_str().unwrap(), home, &[]).is_err());
        assert!(listed_config(&connectors, dir.to_str().unwrap(), home, &[]).is_err());
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
        assert!(listed_config(&connectors, link.to_str().unwrap(), Some(&home), &[]).is_err());
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
            listed_config(&connectors, link.to_str().unwrap(), Some(&home), &[]),
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
        let path = file.to_str().unwrap();
        assert!(listed_config(&connectors, path, Some(&home), &[]).is_err());
        assert!(listed_config(&connectors, path, None, &[]).is_err());
    }

    #[test]
    fn config_open_refuses_a_project_symlink_to_elsewhere_under_home() {
        // <cwd>/.mcp.json -> ~/.ssh/id_rsa: inside home, but not inside the project
        let (_home, home) = root();
        let cwd = home.join("repo");
        let ssh = home.join(".ssh");
        std::fs::create_dir_all(&cwd).unwrap();
        std::fs::create_dir_all(&ssh).unwrap();
        let key = ssh.join("id_rsa");
        std::fs::write(&key, "x").unwrap();
        let link = cwd.join(".mcp.json");
        std::os::unix::fs::symlink(&key, &link).unwrap();
        let connectors = [connector(link.to_str().unwrap())];
        let path = link.to_str().unwrap();
        assert!(listed_config(&connectors, path, Some(&home), &[cwd]).is_err());
    }

    #[test]
    fn config_open_accepts_a_project_file_and_a_symlink_within_the_project() {
        let (_home, home) = root();
        let cwd = home.join("repo");
        std::fs::create_dir_all(&cwd).unwrap();
        let real = cwd.join("mcp.real.json");
        std::fs::write(&real, "{}").unwrap();
        let link = cwd.join(".mcp.json");
        std::os::unix::fs::symlink(&real, &link).unwrap();
        let connectors = [connector(link.to_str().unwrap())];
        let path = link.to_str().unwrap();
        assert_eq!(
            listed_config(&connectors, path, Some(&home), std::slice::from_ref(&cwd)),
            Ok(real)
        );
    }

    #[test]
    fn config_open_refuses_a_project_symlink_when_the_cwd_is_root() {
        // a session whose cwd is `/` must not make the whole disk "project"
        let (_home, home) = root();
        let (_outside, outside) = root();
        let secret = outside.join("id_rsa");
        std::fs::write(&secret, "x").unwrap();
        let link = home.join(".mcp.json");
        std::os::unix::fs::symlink(&secret, &link).unwrap();
        let connectors = [connector(link.to_str().unwrap())];
        let path = link.to_str().unwrap();
        assert!(listed_config(&connectors, path, Some(&home), &[PathBuf::from("/")]).is_err());
        let direct = outside.join("settings.json");
        std::fs::write(&direct, "{}").unwrap();
        let connectors = [connector(direct.to_str().unwrap())];
        assert!(
            listed_config(
                &connectors,
                direct.to_str().unwrap(),
                None,
                &[PathBuf::from("/")]
            )
            .is_err()
        );
    }
}
