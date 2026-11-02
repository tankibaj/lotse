//! Cheap git facts that need no `git` process; the rest lives in [`crate::git_glue`].

use std::path::Path;

/// `.git` (directory, or file for worktrees and submodules) in `dir` or one of its parents.
pub fn is_git_repo(dir: &Path) -> bool {
    dir.ancestors().any(|d| d.join(".git").exists())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dot_git_makes_a_folder_and_its_children_a_repo() {
        let tmp = tempfile::tempdir().unwrap();
        let repo = tmp.path().join("r");
        std::fs::create_dir_all(repo.join(".git")).unwrap();
        std::fs::create_dir_all(repo.join("sub")).unwrap();
        assert!(is_git_repo(&repo));
        assert!(is_git_repo(&repo.join("sub")));
    }
}
