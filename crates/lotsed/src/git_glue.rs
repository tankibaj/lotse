//! `lotse-git` wired into the daemon: git line of a session, isolated worktrees.

use crate::git::is_git_repo;
use crate::launch::LaunchEnv;
use crate::pr;
use lotse_proto::GitInfo;
use std::path::{Path, PathBuf};

/// Branch, ahead, dirty, worktree of `dir`. Runs `git`: call it off the async runtime and never
/// under a lock. When git cannot run, fall back to the cheap `.git` check.
pub fn info(dir: &Path) -> GitInfo {
    lotse_git::git_info(dir).unwrap_or_else(|e| {
        tracing::debug!("git info for {}: {e}", dir.display());
        GitInfo {
            is_git: is_git_repo(dir),
            ..GitInfo::default()
        }
    })
}

/// [`info`] plus the branch's pull request. `pr_env` is `Some` only while Settings › External ›
/// PR status is on; then this may run `gh` / `glab` (see [`crate::pr::status`]: up to 5 s on a
/// cache miss, one miss per branch and minute), so it stays off the async runtime too.
pub fn info_with_pr(dir: &Path, pr_env: Option<&LaunchEnv>) -> GitInfo {
    let mut git = info(dir);
    if git.is_git
        && let Some(env) = pr_env
    {
        git.pr = pr::status(dir, &git.branch, env);
    }
    git
}

/// [`info_with_pr`] for the periodic refresh of every session's git line, which shares one thread
/// and so must not wait for `gh` / `glab`: the PR is whatever [`pr::status_in_background`] has (the
/// lookup runs on its own thread; [`pr::generation`] moves when its answer changes).
pub fn info_with_pr_refresh(dir: &Path, pr_env: Option<&LaunchEnv>) -> GitInfo {
    let mut git = info(dir);
    if git.is_git
        && let Some(env) = pr_env
    {
        git.pr = pr::status_in_background(dir, &git.branch, env);
    }
    git
}

/// The folder a new session runs in: `repo` itself, or a new isolated worktree for `branch`
/// (`~/.lotse/worktrees/<project>/<branch>`).
pub fn session_dir(repo: &Path, branch: Option<&str>) -> Result<PathBuf, lotse_git::GitError> {
    match branch {
        None => Ok(repo.to_path_buf()),
        Some(b) => lotse_git::create_worktree(repo, b),
    }
}

/// Best effort: remove a worktree that a failed launch left behind.
pub fn remove_worktree(repo: &Path, worktree: &Path) {
    let removed = std::process::Command::new("git")
        .arg("-C")
        .arg(repo)
        .args(["worktree", "remove", "--force", "--"])
        .arg(worktree)
        .stdin(std::process::Stdio::null())
        .output();
    if !removed.is_ok_and(|o| o.status.success()) {
        tracing::warn!("could not remove worktree {}", worktree.display());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::Command;

    fn git(dir: &Path, args: &[&str]) {
        let ok = Command::new("git")
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
            .success();
        assert!(ok, "git {args:?}");
    }

    #[test]
    fn plain_folder_is_no_git_and_a_repo_reports_branch_and_dirty() {
        let tmp = tempfile::tempdir().unwrap();
        let plain = tmp.path().join("plain");
        std::fs::create_dir(&plain).unwrap();
        assert!(!info(&plain).is_git);

        let repo = tmp.path().join("repo");
        std::fs::create_dir(&repo).unwrap();
        git(&repo, &["init", "-q", "-b", "trunk"]);
        git(&repo, &["commit", "-q", "--allow-empty", "-m", "init"]);
        std::fs::write(repo.join("a.txt"), "x").unwrap();
        let i = info(&repo);
        assert!(i.is_git);
        assert_eq!(i.branch, "trunk");
        assert_eq!(i.dirty, 1);
        assert!(!i.worktree);
    }

    #[test]
    fn a_worktree_left_by_a_failed_launch_can_be_removed_and_its_branch_reused() {
        let tmp = tempfile::tempdir().unwrap();
        let repo = tmp.path().join("repo");
        std::fs::create_dir(&repo).unwrap();
        git(&repo, &["init", "-q", "-b", "main"]);
        git(&repo, &["commit", "-q", "--allow-empty", "-m", "init"]);
        let root = tmp.path().join("wts");
        let wt = lotse_git::create_worktree_in(&repo, "lotse/x", &root).unwrap();
        assert!(wt.is_dir());
        remove_worktree(&repo, &wt);
        assert!(!wt.exists());
        // the same name works again
        assert!(
            lotse_git::create_worktree_in(&repo, "lotse/x", &root)
                .unwrap()
                .is_dir()
        );
    }

    #[test]
    fn no_branch_means_the_repo_itself() {
        let p = Path::new("/tmp/x");
        assert_eq!(session_dir(p, None).unwrap(), p);
    }
}
