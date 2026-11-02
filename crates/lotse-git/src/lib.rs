//! Git info for sessions: branch, ahead, dirty, worktree; worktree creation (WP7).
//!
//! Everything goes through the `git` CLI (no libgit2). The result type is the protocol's
//! [`GitInfo`], so the daemon can forward it unchanged.

use std::ffi::OsStr;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

pub use lotse_proto::GitInfo;

#[derive(Debug, thiserror::Error)]
pub enum GitError {
    #[error("git not found on PATH")]
    GitMissing,
    #[error("git failed: {0}")]
    Command(String),
    #[error("not a directory: {0}")]
    NotADirectory(PathBuf),
    #[error("not a git repository: {0}")]
    NotARepo(PathBuf),
    #[error("invalid branch name: {0}")]
    InvalidBranch(String),
    #[error("worktree path already exists: {0}")]
    PathExists(PathBuf),
    #[error("cannot resolve the home directory")]
    NoHome,
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
}

/// Runs `git -C dir args…` with a stable environment: English output, no prompts, and no
/// optional index locks (the status refresh must never race an agent's own `git` calls).
fn run<I, S>(dir: &Path, args: I) -> Result<Output, GitError>
where
    I: IntoIterator<Item = S>,
    S: AsRef<OsStr>,
{
    Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(args)
        .env("LC_ALL", "C")
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("GIT_OPTIONAL_LOCKS", "0")
        .env_remove("GIT_DIR")
        .env_remove("GIT_WORK_TREE")
        .env_remove("GIT_INDEX_FILE")
        .output()
        .map_err(|e| match e.kind() {
            std::io::ErrorKind::NotFound => GitError::GitMissing,
            _ => GitError::Io(e),
        })
}

/// Stdout of a successful command, trimmed; `None` when git exited non-zero.
fn out_ok<I, S>(dir: &Path, args: I) -> Result<Option<String>, GitError>
where
    I: IntoIterator<Item = S>,
    S: AsRef<OsStr>,
{
    let o = run(dir, args)?;
    Ok(o.status
        .success()
        .then(|| String::from_utf8_lossy(&o.stdout).trim().to_string()))
}

fn stderr(o: &Output) -> String {
    String::from_utf8_lossy(&o.stderr).trim().to_string()
}

fn no_git() -> GitInfo {
    GitInfo {
        is_git: false,
        branch: String::new(),
        ahead: 0,
        dirty: 0,
        worktree: false,
        worktree_path: None,
    }
}

/// Where `dir` sits in a repository: work-tree root, own git dir, shared git dir.
struct Layout {
    toplevel: PathBuf,
    git_dir: PathBuf,
    common_dir: PathBuf,
}

/// `None` when `dir` is outside any work tree (plain folder, bare repo, `.git` internals).
fn layout(dir: &Path) -> Result<Option<Layout>, GitError> {
    if !dir.is_dir() {
        return Err(GitError::NotADirectory(dir.to_path_buf()));
    }
    let o = run(
        dir,
        [
            "rev-parse",
            "--show-toplevel",
            "--git-dir",
            "--git-common-dir",
        ],
    )?;
    if !o.status.success() {
        let err = stderr(&o);
        if err.contains("not a git repository") || err.contains("must be run in a work tree") {
            return Ok(None);
        }
        return Err(GitError::Command(err));
    }
    let text = String::from_utf8_lossy(&o.stdout);
    let mut lines = text.lines();
    let (Some(top), Some(git), Some(common)) = (lines.next(), lines.next(), lines.next()) else {
        return Err(GitError::Command(format!(
            "unexpected rev-parse output: {text}"
        )));
    };
    // `--git-dir` and `--git-common-dir` are relative to `dir` when not absolute.
    let resolve = |p: &str| dir.join(p).canonicalize();
    Ok(Some(Layout {
        toplevel: PathBuf::from(top),
        git_dir: resolve(git)?,
        common_dir: resolve(common)?,
    }))
}

/// Branch, commits ahead, uncommitted changes and worktree flag for `dir`.
/// A folder outside any repository yields `GitInfo { is_git: false, .. }` ("No git").
///
/// - `branch`: empty on a detached HEAD; the unborn branch name in a fresh repo.
/// - `ahead`: commits not on the upstream branch; 0 without an upstream.
/// - `dirty`: lines of `git status --porcelain` (modified, staged and untracked entries).
/// - `worktree`: true in a linked worktree (`git worktree add`), false in the main checkout.
pub fn git_info(dir: &Path) -> Result<GitInfo, GitError> {
    let Some(l) = layout(dir)? else {
        return Ok(no_git());
    };
    // `branch --show-current`, not `symbolic-ref --short`: the latter prints `heads/x` when a tag `x` exists.
    let branch = out_ok(dir, ["branch", "--show-current"])?.unwrap_or_default();
    let ahead = out_ok(dir, ["rev-list", "--count", "@{upstream}..HEAD"])?
        .and_then(|s| s.parse().ok())
        .unwrap_or(0);
    let status = run(dir, ["status", "--porcelain"])?;
    if !status.status.success() {
        return Err(GitError::Command(stderr(&status)));
    }
    let dirty = String::from_utf8_lossy(&status.stdout)
        .lines()
        .filter(|l| !l.is_empty())
        .count() as u32;
    let worktree = l.git_dir != l.common_dir;
    Ok(GitInfo {
        is_git: true,
        branch,
        ahead,
        dirty,
        worktree,
        worktree_path: worktree.then(|| l.toplevel.to_string_lossy().into_owned()),
    })
}

/// `~/.lotse/worktrees`, or `None` without a home directory.
pub fn worktrees_root() -> Option<PathBuf> {
    dirs::home_dir().map(|h| h.join(".lotse").join("worktrees"))
}

/// Create an isolated worktree for `branch` under `~/.lotse/worktrees/<project>/<branch>`.
/// Creates the branch from the current HEAD when it does not exist yet. Returns the new path.
/// A leading `lotse/` is left out of the folder name (the mock's tooltip), not of the branch.
pub fn create_worktree(repo: &Path, branch: &str) -> Result<PathBuf, GitError> {
    let root = worktrees_root().ok_or(GitError::NoHome)?;
    create_worktree_in(repo, branch, &root)
}

/// [`create_worktree`] with an explicit root (`<root>/<project>/<branch>`); tests use a temp dir.
pub fn create_worktree_in(repo: &Path, branch: &str, root: &Path) -> Result<PathBuf, GitError> {
    // `refs/heads/<branch>` must be a valid ref: this rules out `..`, `@{`, spaces, control
    // characters and a trailing `.lock`, so the path below cannot escape `<root>/<project>`.
    // A leading `-` is refused too: it is legal in a ref but reads as an option elsewhere.
    let full = format!("refs/heads/{branch}");
    let valid = !branch.is_empty()
        && !branch.starts_with(['-', '/'])
        && run(repo, ["check-ref-format", full.as_str()])?
            .status
            .success();
    if !valid {
        return Err(GitError::InvalidBranch(branch.to_string()));
    }
    let l = layout(repo)?.ok_or_else(|| GitError::NotARepo(repo.to_path_buf()))?;
    // The mock's tooltip shows `~/.lotse/worktrees/<project>/<branch without "lotse/">`.
    let dir = branch.strip_prefix("lotse/").unwrap_or(branch);
    let dest = root.join(project_name(&l)).join(dir);
    if dest.exists() {
        return Err(GitError::PathExists(dest));
    }
    // `git worktree add` creates the missing parent folders itself (and removes them on failure).
    let exists = run(repo, ["show-ref", "--verify", "--quiet", full.as_str()])?
        .status
        .success();
    let o = if exists {
        run(
            repo,
            [
                OsStr::new("worktree"),
                "add".as_ref(),
                dest.as_os_str(),
                branch.as_ref(),
            ],
        )?
    } else {
        run(
            repo,
            [
                OsStr::new("worktree"),
                "add".as_ref(),
                "-b".as_ref(),
                branch.as_ref(),
                dest.as_os_str(),
            ],
        )?
    };
    if !o.status.success() {
        return Err(GitError::Command(stderr(&o)));
    }
    Ok(dest.canonicalize()?)
}

/// Folder name of the main checkout (also when `repo` is itself a linked worktree), made path-safe.
fn project_name(l: &Layout) -> String {
    let main = if l.common_dir.file_name() == Some(OsStr::new(".git")) {
        l.common_dir.parent().unwrap_or(&l.toplevel)
    } else {
        &l.toplevel
    };
    let raw = main
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let safe: String = raw
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-') {
                c
            } else {
                '-'
            }
        })
        .collect();
    let safe = safe.trim_start_matches('.');
    if safe.is_empty() {
        "project".to_string()
    } else {
        safe.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    /// Runs git in `dir` with a hermetic config (no user config, fixed identity, no signing).
    fn git(dir: &Path, args: &[&str]) {
        let o = Command::new("git")
            .arg("-C")
            .arg(dir)
            .args([
                "-c",
                "commit.gpgsign=false",
                "-c",
                "protocol.file.allow=always",
            ])
            .args(args)
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_SYSTEM", "/dev/null")
            .env("GIT_AUTHOR_NAME", "t")
            .env("GIT_AUTHOR_EMAIL", "t@example.com")
            .env("GIT_COMMITTER_NAME", "t")
            .env("GIT_COMMITTER_EMAIL", "t@example.com")
            .output()
            .expect("git runs");
        assert!(o.status.success(), "git {args:?}: {}", stderr(&o));
    }

    fn write(dir: &Path, name: &str, text: &str) {
        std::fs::write(dir.join(name), text).unwrap();
    }

    /// A repo on `main` with one commit.
    fn repo() -> TempDir {
        let d = tempfile::tempdir().unwrap();
        git(d.path(), &["init", "-q", "-b", "main"]);
        write(d.path(), "a.txt", "a\n");
        git(d.path(), &["add", "."]);
        git(d.path(), &["commit", "-q", "-m", "init"]);
        d
    }

    fn real(p: &Path) -> PathBuf {
        p.canonicalize().unwrap()
    }

    #[test]
    fn plain_folder_is_no_git() {
        let dir = tempfile::tempdir().unwrap();
        let info = git_info(dir.path()).unwrap();
        assert!(!info.is_git);
        assert_eq!(
            (info.branch.as_str(), info.ahead, info.dirty, info.worktree),
            ("", 0, 0, false)
        );
    }

    #[test]
    fn missing_dir_is_an_error() {
        let dir = tempfile::tempdir().unwrap();
        let gone = dir.path().join("gone");
        assert!(matches!(git_info(&gone), Err(GitError::NotADirectory(_))));
    }

    #[test]
    fn clean_repo_reports_branch() {
        let d = repo();
        let info = git_info(d.path()).unwrap();
        assert!(info.is_git);
        assert_eq!(info.branch, "main");
        assert_eq!((info.ahead, info.dirty, info.worktree), (0, 0, false));
        assert_eq!(info.worktree_path, None);
    }

    #[test]
    fn subdirectory_is_part_of_the_repo() {
        let d = repo();
        std::fs::create_dir(d.path().join("sub")).unwrap();
        let info = git_info(&d.path().join("sub")).unwrap();
        assert!(info.is_git && !info.worktree);
    }

    #[test]
    fn fresh_repo_without_commits_has_branch_and_no_ahead() {
        let d = tempfile::tempdir().unwrap();
        git(d.path(), &["init", "-q", "-b", "trunk"]);
        let info = git_info(d.path()).unwrap();
        assert!(info.is_git);
        assert_eq!((info.branch.as_str(), info.ahead), ("trunk", 0));
    }

    #[test]
    fn dirty_counts_modified_staged_and_untracked() {
        let d = repo();
        write(d.path(), "a.txt", "changed\n"); // modified
        write(d.path(), "b.txt", "new\n"); // untracked
        write(d.path(), "c.txt", "staged\n");
        git(d.path(), &["add", "c.txt"]); // staged
        assert_eq!(git_info(d.path()).unwrap().dirty, 3);
    }

    #[test]
    fn ahead_counts_commits_not_on_upstream() {
        let origin = tempfile::tempdir().unwrap();
        git(origin.path(), &["init", "-q", "--bare", "-b", "main"]);
        let d = repo();
        let url = origin.path().to_str().unwrap();
        git(d.path(), &["remote", "add", "origin", url]);
        git(d.path(), &["push", "-q", "-u", "origin", "main"]);
        assert_eq!(git_info(d.path()).unwrap().ahead, 0);
        for n in 1..=2 {
            write(d.path(), "a.txt", &format!("v{n}\n"));
            git(d.path(), &["commit", "-q", "-am", &format!("c{n}")]);
        }
        let info = git_info(d.path()).unwrap();
        assert_eq!((info.ahead, info.dirty), (2, 0));
    }

    #[test]
    fn detached_head_has_empty_branch() {
        let d = repo();
        git(d.path(), &["checkout", "-q", "--detach"]);
        let info = git_info(d.path()).unwrap();
        assert!(info.is_git);
        assert_eq!(info.branch, "");
    }

    #[test]
    fn bare_repo_is_no_git() {
        let d = tempfile::tempdir().unwrap();
        git(d.path(), &["init", "-q", "--bare"]);
        assert!(!git_info(d.path()).unwrap().is_git);
    }

    #[test]
    fn worktree_add_creates_branch_under_root_and_is_detected() {
        let d = repo();
        let root = tempfile::tempdir().unwrap();
        let path = create_worktree_in(d.path(), "feature/x", root.path()).unwrap();
        // Temp dir names may start with a dot; `project_name` drops leading dots.
        let project = d
            .path()
            .file_name()
            .unwrap()
            .to_str()
            .unwrap()
            .trim_start_matches('.');
        assert_eq!(path, real(root.path()).join(project).join("feature/x"));
        assert!(path.join("a.txt").exists());

        let info = git_info(&path).unwrap();
        assert!(info.is_git && info.worktree);
        assert_eq!(info.branch, "feature/x");
        assert_eq!(info.worktree_path.as_deref(), path.to_str());
        // The main checkout stays a plain checkout on its own branch.
        let main = git_info(d.path()).unwrap();
        assert!(!main.worktree);
        assert_eq!(main.branch, "main");
    }

    #[test]
    fn worktree_path_drops_the_lotse_branch_prefix_like_the_mock() {
        let d = repo();
        let root = tempfile::tempdir().unwrap();
        let path = create_worktree_in(d.path(), "lotse/fix-login", root.path()).unwrap();
        assert!(path.ends_with("fix-login"), "{path:?}");
        assert_eq!(git_info(&path).unwrap().branch, "lotse/fix-login");
    }

    #[test]
    fn worktree_dirty_is_counted_per_worktree() {
        let d = repo();
        let root = tempfile::tempdir().unwrap();
        let path = create_worktree_in(d.path(), "wt", root.path()).unwrap();
        write(&path, "only-here.txt", "x\n");
        assert_eq!(git_info(&path).unwrap().dirty, 1);
        assert_eq!(git_info(d.path()).unwrap().dirty, 0);
    }

    #[test]
    fn worktree_reuses_an_existing_branch() {
        let d = repo();
        git(d.path(), &["branch", "existing"]);
        let root = tempfile::tempdir().unwrap();
        let path = create_worktree_in(d.path(), "existing", root.path()).unwrap();
        assert_eq!(git_info(&path).unwrap().branch, "existing");
    }

    #[test]
    fn worktree_from_a_linked_worktree_uses_the_main_project_name() {
        let d = repo();
        let root = tempfile::tempdir().unwrap();
        let first = create_worktree_in(d.path(), "one", root.path()).unwrap();
        let second = create_worktree_in(&first, "two", root.path()).unwrap();
        assert_eq!(first.parent().unwrap(), second.parent().unwrap());
    }

    #[test]
    fn worktree_refuses_existing_path_and_checked_out_branch() {
        let d = repo();
        let root = tempfile::tempdir().unwrap();
        create_worktree_in(d.path(), "dup", root.path()).unwrap();
        assert!(matches!(
            create_worktree_in(d.path(), "dup", root.path()),
            Err(GitError::PathExists(_))
        ));
        // `main` is checked out in the main checkout: git refuses, the error carries its message.
        assert!(matches!(
            create_worktree_in(d.path(), "main", root.path()),
            Err(GitError::Command(_))
        ));
    }

    #[test]
    fn failed_worktree_add_leaves_nothing_behind() {
        let d = repo();
        let root = tempfile::tempdir().unwrap();
        assert!(create_worktree_in(d.path(), "main", root.path()).is_err());
        assert_eq!(std::fs::read_dir(root.path()).unwrap().count(), 0);
    }

    #[test]
    fn branch_is_right_when_a_tag_has_the_same_name() {
        let d = repo();
        git(d.path(), &["tag", "main"]);
        assert_eq!(git_info(d.path()).unwrap().branch, "main");
    }

    #[test]
    fn worktree_rejects_unsafe_branch_names() {
        let d = repo();
        let root = tempfile::tempdir().unwrap();
        for bad in [
            "",
            "../escape",
            "a/../b",
            "/abs",
            "-x",
            "a b",
            "x.lock",
            "@{-1}",
            "a..b",
        ] {
            assert!(
                matches!(
                    create_worktree_in(d.path(), bad, root.path()),
                    Err(GitError::InvalidBranch(_))
                ),
                "{bad:?} must be rejected"
            );
        }
        assert_eq!(
            std::fs::read_dir(root.path()).unwrap().count(),
            0,
            "nothing created"
        );
    }

    #[test]
    fn worktree_outside_a_repo_is_an_error() {
        let plain = tempfile::tempdir().unwrap();
        let root = tempfile::tempdir().unwrap();
        assert!(matches!(
            create_worktree_in(plain.path(), "x", root.path()),
            Err(GitError::NotARepo(_))
        ));
    }

    #[test]
    fn project_name_is_path_safe() {
        let l = |p: &str| Layout {
            toplevel: p.into(),
            git_dir: p.into(),
            common_dir: format!("{p}/.git").into(),
        };
        assert_eq!(project_name(&l("/x/praxis-web")), "praxis-web");
        assert_eq!(project_name(&l("/x/my proj")), "my-proj");
        assert_eq!(project_name(&l("/x/.hidden")), "hidden");
        assert_eq!(project_name(&l("/")), "project");
    }
}
