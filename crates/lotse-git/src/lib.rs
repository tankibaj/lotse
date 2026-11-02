//! Git info for sessions: branch, ahead, dirty, worktree; worktree creation (WP7).
//!
//! The result type is the protocol's [`GitInfo`], so the daemon can forward it unchanged.

use std::path::Path;

pub use lotse_proto::GitInfo;

#[derive(Debug, thiserror::Error)]
pub enum GitError {
    #[error("git not found on PATH")]
    GitMissing,
    #[error("git failed: {0}")]
    Command(String),
}

/// Branch, commits ahead, uncommitted changes and worktree flag for `dir`.
/// A folder outside any repository yields `GitInfo { is_git: false, .. }` ("No git").
pub fn git_info(dir: &Path) -> Result<GitInfo, GitError> {
    let _ = dir;
    todo!("WP7")
}

/// Create an isolated worktree for `branch` under `~/.lotse/worktrees/<project>/<branch>`.
pub fn create_worktree(repo: &Path, branch: &str) -> Result<std::path::PathBuf, GitError> {
    let _ = (repo, branch);
    todo!("WP7")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[ignore = "WP7: git helpers not implemented"]
    fn plain_folder_is_no_git() {
        let dir = tempfile::tempdir().unwrap();
        assert!(!git_info(dir.path()).unwrap().is_git);
    }
}
