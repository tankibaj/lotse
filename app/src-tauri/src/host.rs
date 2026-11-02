//! Host commands the frozen `DaemonClient` has no calls for: repository scan, clone, project folder
//! creation (Add project sheet), opening links and Reveal in Finder, and the agent integrations
//! (Claude hooks, OpenCode plugin) of Settings › Agents.
//!
//! Every path and URL comes from the webview, so each command validates its input and passes user
//! values to child processes as separate arguments (never through a shell), after `--` where the
//! program has one.

use crate::daemon::sibling_binary;
use serde::Serialize;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

const HOOK_BIN_ENV: &str = "LOTSE_HOOK_BIN";
/// Most child folders looked at when scanning a folder for repositories.
const SCAN_LIMIT: usize = 500;

#[derive(Debug, Serialize, PartialEq, Eq)]
pub struct RepoScan {
    /// The folder itself is a git repository.
    pub is_repo: bool,
    /// The folder itself, or its direct children that are repositories.
    pub repos: Vec<String>,
}

/// `~` and `~/…` → the home directory; anything else must already be absolute.
pub fn expand_path(raw: &str, home: Option<&Path>) -> Result<PathBuf, String> {
    let raw = raw.trim();
    if raw.is_empty() || raw.contains('\0') {
        return Err("empty path".into());
    }
    let path = if raw == "~" {
        home.ok_or("cannot find the home directory")?.to_path_buf()
    } else if let Some(rest) = raw.strip_prefix("~/") {
        home.ok_or("cannot find the home directory")?.join(rest)
    } else {
        PathBuf::from(raw)
    };
    if !path.is_absolute() {
        return Err("path must be absolute".into());
    }
    Ok(path)
}

fn expand(raw: &str) -> Result<PathBuf, String> {
    expand_path(raw, dirs::home_dir().as_deref())
}

fn is_repo(dir: &Path) -> bool {
    dir.join(".git").exists()
}

pub fn scan(dir: &Path) -> Result<RepoScan, String> {
    if !dir.is_dir() {
        return Err(format!("{} is not a folder", dir.display()));
    }
    if is_repo(dir) {
        return Ok(RepoScan {
            is_repo: true,
            repos: vec![dir.to_string_lossy().into_owned()],
        });
    }
    let mut repos: Vec<String> = std::fs::read_dir(dir)
        .map_err(|e| format!("reading {}: {e}", dir.display()))?
        .filter_map(Result::ok)
        .take(SCAN_LIMIT)
        .map(|e| e.path())
        .filter(|p| p.is_dir() && is_repo(p))
        .map(|p| p.to_string_lossy().into_owned())
        .collect();
    repos.sort();
    Ok(RepoScan {
        is_repo: false,
        repos,
    })
}

#[tauri::command]
pub fn scan_repos(path: String) -> Result<RepoScan, String> {
    scan(&expand(&path)?)
}

/// A clone URL must not look like an option or smuggle whitespace or control characters.
pub fn check_clone_url(url: &str) -> Result<&str, String> {
    let url = url.trim();
    if url.is_empty() || url.starts_with('-') || url.chars().any(|c| c.is_control() || c == ' ') {
        return Err("invalid repository URL".into());
    }
    Ok(url)
}

/// `git clone -- <url> <dest>`; the destination must not exist (or be an empty folder).
pub fn clone_into(url: &str, dest: &Path) -> Result<(), String> {
    let url = check_clone_url(url)?;
    if dest.exists() && !(dest.is_dir() && dest.read_dir().is_ok_and(|mut d| d.next().is_none())) {
        return Err(format!("{} already exists", dest.display()));
    }
    let out = Command::new("git")
        .args(["clone", "--quiet", "--"])
        .arg(url)
        .arg(dest)
        // no prompts, and no `ext::` transport (it runs arbitrary commands)
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("GIT_ALLOW_PROTOCOL", "file:git:http:https:ssh")
        .stdin(Stdio::null())
        .output()
        .map_err(|e| format!("running git: {e}"))?;
    if out.status.success() {
        return Ok(());
    }
    let err = String::from_utf8_lossy(&out.stderr);
    Err(err
        .lines()
        .rev()
        .find(|l| !l.trim().is_empty())
        .unwrap_or("git clone failed")
        .trim()
        .to_owned())
}

#[tauri::command]
pub async fn clone_repo(url: String, dest: String) -> Result<(), String> {
    let dest = expand(&dest)?;
    tauri::async_runtime::spawn_blocking(move || clone_into(&url, &dest))
        .await
        .map_err(|e| e.to_string())?
}

/// Create the folder (and its parents); `git init` it when asked. An existing folder is fine.
pub fn create_dir(path: &Path, git_init: bool) -> Result<(), String> {
    std::fs::create_dir_all(path).map_err(|e| format!("creating {}: {e}", path.display()))?;
    if git_init && !is_repo(path) {
        let out = Command::new("git")
            .args(["init", "--quiet"])
            .current_dir(path)
            .stdin(Stdio::null())
            .output()
            .map_err(|e| format!("running git: {e}"))?;
        if !out.status.success() {
            return Err(String::from_utf8_lossy(&out.stderr).trim().to_owned());
        }
    }
    Ok(())
}

#[tauri::command]
pub async fn create_project_dir(path: String, git_init: bool) -> Result<(), String> {
    let path = expand(&path)?;
    tauri::async_runtime::spawn_blocking(move || create_dir(&path, git_init))
        .await
        .map_err(|e| e.to_string())?
}

// ---- links and Finder ---------------------------------------------------------------------------

/// Only `http` and `https` (what the terminal link provider hands over), no whitespace or control
/// characters: the URL becomes one argument of `/usr/bin/open`.
pub fn check_open_url(url: &str) -> Result<&str, String> {
    let url = url.trim();
    let lower = url.to_ascii_lowercase();
    let ok_scheme = lower.starts_with("http://") || lower.starts_with("https://");
    if !ok_scheme || url.len() > 4096 || url.chars().any(|c| c.is_control() || c == ' ') {
        return Err("only http and https links can be opened".into());
    }
    Ok(url)
}

#[tauri::command]
pub fn open_url(url: String) -> Result<(), String> {
    let url = check_open_url(&url)?;
    spawn_open(Command::new("/usr/bin/open").arg(url))
}

/// Show a file or folder in Finder (`open -R`).
#[tauri::command]
pub fn reveal_in_finder(path: String) -> Result<(), String> {
    let path = expand(&path)?;
    if !path.exists() {
        return Err(format!("{} does not exist", path.display()));
    }
    spawn_open(Command::new("/usr/bin/open").arg("-R").arg(&path))
}

fn spawn_open(cmd: &mut Command) -> Result<(), String> {
    let mut child = cmd
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

// ---- agent integrations -------------------------------------------------------------------------

fn check_integration(agent: &str, action: &str) -> Result<(), String> {
    if !matches!(agent, "claude" | "opencode") {
        return Err(format!("unknown integration {agent:?}"));
    }
    if !matches!(action, "install" | "uninstall" | "status") {
        return Err(format!("unknown action {action:?}"));
    }
    Ok(())
}

/// `lotse-hook integration <agent> <action>` (the hook binary ships next to the app).
fn run_integration(agent: &str, action: &str) -> Result<String, String> {
    check_integration(agent, action)?;
    let bin = sibling_binary("lotse-hook", HOOK_BIN_ENV)
        .ok_or("lotse-hook not found next to the app; build it (`cargo build -p lotse-hook`)")?;
    let out = Command::new(&bin)
        .args(["integration", agent, action])
        .stdin(Stdio::null())
        .output()
        .map_err(|e| format!("running {}: {e}", bin.display()))?;
    let text = String::from_utf8_lossy(&out.stdout).trim().to_owned();
    if out.status.success() {
        Ok(text)
    } else {
        Err(String::from_utf8_lossy(&out.stderr)
            .lines()
            .next()
            .unwrap_or("integration failed")
            .to_owned())
    }
}

/// `installed`, `not installed`, or `partial…`/`outdated…`.
#[tauri::command]
pub async fn integration_status(agent: String) -> Result<String, String> {
    tauri::async_runtime::spawn_blocking(move || run_integration(&agent, "status"))
        .await
        .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn integration_install(agent: String) -> Result<String, String> {
    tauri::async_runtime::spawn_blocking(move || run_integration(&agent, "install"))
        .await
        .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn integration_uninstall(agent: String) -> Result<String, String> {
    tauri::async_runtime::spawn_blocking(move || run_integration(&agent, "uninstall"))
        .await
        .map_err(|e| e.to_string())?
}

#[cfg(test)]
mod tests {
    use super::*;

    fn git(dir: &Path, args: &[&str]) {
        assert!(
            Command::new("git")
                .args(args)
                .current_dir(dir)
                .env("GIT_CONFIG_GLOBAL", "/dev/null")
                .env("GIT_CONFIG_SYSTEM", "/dev/null")
                .env("GIT_AUTHOR_NAME", "t")
                .env("GIT_AUTHOR_EMAIL", "t@t")
                .env("GIT_COMMITTER_NAME", "t")
                .env("GIT_COMMITTER_EMAIL", "t@t")
                .status()
                .unwrap()
                .success()
        );
    }

    #[test]
    fn tilde_expands_and_relative_paths_are_refused() {
        let home = Path::new("/Users/x");
        assert_eq!(
            expand_path("~/code/a", Some(home)).unwrap(),
            Path::new("/Users/x/code/a")
        );
        assert_eq!(expand_path("~", Some(home)).unwrap(), home);
        assert_eq!(expand_path("/abs/p", None).unwrap(), Path::new("/abs/p"));
        assert!(expand_path("rel/p", Some(home)).is_err());
        assert!(expand_path("  ", Some(home)).is_err());
        assert!(expand_path("~/x", None).is_err());
    }

    #[test]
    fn scan_finds_the_repo_itself_or_its_child_repos() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        for n in ["api", "web"] {
            std::fs::create_dir_all(root.join(n).join(".git")).unwrap();
        }
        std::fs::create_dir_all(root.join("docs")).unwrap();
        std::fs::write(root.join("file.txt"), "x").unwrap();
        let s = scan(root).unwrap();
        assert!(!s.is_repo);
        assert_eq!(
            s.repos,
            vec![
                root.join("api").to_string_lossy().into_owned(),
                root.join("web").to_string_lossy().into_owned()
            ]
        );
        let own = scan(&root.join("api")).unwrap();
        assert!(own.is_repo);
        assert_eq!(own.repos.len(), 1);
        assert!(scan(&root.join("file.txt")).is_err());
    }

    #[test]
    fn create_dir_makes_parents_and_optionally_a_repo() {
        let tmp = tempfile::tempdir().unwrap();
        let p = tmp.path().join("a/b/new-project");
        create_dir(&p, true).unwrap();
        assert!(p.join(".git").exists());
        // again: an existing repository is left alone
        create_dir(&p, true).unwrap();
        let plain = tmp.path().join("plain");
        create_dir(&plain, false).unwrap();
        assert!(plain.is_dir() && !plain.join(".git").exists());
    }

    #[test]
    fn clone_urls_cannot_be_options_or_carry_whitespace() {
        assert!(check_clone_url("--upload-pack=touch /tmp/x").is_err());
        assert!(check_clone_url("-c core.sshCommand=x").is_err());
        assert!(check_clone_url("https://x.dev/a b.git").is_err());
        assert!(check_clone_url("https://x.dev/a\n.git").is_err());
        assert!(check_clone_url("").is_err());
        assert_eq!(
            check_clone_url(" https://x.dev/a.git ").unwrap(),
            "https://x.dev/a.git"
        );
    }

    #[test]
    fn clone_copies_a_local_repo_and_refuses_an_existing_destination() {
        let tmp = tempfile::tempdir().unwrap();
        let src = tmp.path().join("src");
        std::fs::create_dir(&src).unwrap();
        git(&src, &["init", "-q", "-b", "main"]);
        git(&src, &["commit", "-q", "--allow-empty", "-m", "init"]);
        let dest = tmp.path().join("dest");
        clone_into(src.to_str().unwrap(), &dest).unwrap();
        assert!(dest.join(".git").exists());
        let err = clone_into(src.to_str().unwrap(), &dest).unwrap_err();
        assert!(err.contains("already exists"));
        // an `ext::` URL never reaches a command: the protocol is not allowed
        let evil = tmp.path().join("evil");
        let marker = tmp.path().join("pwned");
        let url = format!("ext::sh -c touch% {}", marker.display());
        assert!(clone_into(&url, &evil).is_err());
        assert!(!marker.exists());
    }

    #[test]
    fn only_http_and_https_links_open() {
        assert!(check_open_url("https://example.com/a?b=c").is_ok());
        assert!(check_open_url("HTTP://example.com").is_ok());
        for bad in [
            "file:///etc/passwd",
            "javascript:alert(1)",
            "ssh://host",
            "-a Calculator",
            "https://x.dev/a b",
            "https://x.dev/\nfoo",
            "mailto:a@b.c",
        ] {
            assert!(check_open_url(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn integration_names_are_whitelisted() {
        assert!(check_integration("claude", "install").is_ok());
        assert!(check_integration("opencode", "status").is_ok());
        assert!(check_integration("claude; rm -rf /", "install").is_err());
        assert!(check_integration("claude", "../x").is_err());
    }
}
