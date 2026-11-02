//! Git facts for projects and sessions.
//!
//! `lotse_git::git_info` is still a `todo!()` stub (WP7), so the daemon only answers what it can
//! know without running git: whether the folder is a repository. WP8 swaps this for `lotse_git`.

use lotse_proto::GitInfo;
use std::path::Path;

/// `.git` (directory, or file for worktrees and submodules) in `dir` or one of its parents.
pub fn is_git_repo(dir: &Path) -> bool {
    dir.ancestors().any(|d| d.join(".git").exists())
}

pub fn info(dir: &Path) -> GitInfo {
    GitInfo {
        is_git: is_git_repo(dir),
        branch: String::new(),
        ahead: 0,
        dirty: 0,
        worktree: false,
        worktree_path: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plain_folder_is_not_git_and_dot_git_makes_it_git() {
        let tmp = tempfile::tempdir().unwrap();
        // a bare temp dir is not inside a repository
        assert!(!info(tmp.path()).is_git);
        let repo = tmp.path().join("r");
        std::fs::create_dir_all(repo.join(".git")).unwrap();
        std::fs::create_dir_all(repo.join("sub")).unwrap();
        assert!(info(&repo).is_git);
        assert!(info(&repo.join("sub")).is_git);
    }
}
