//! Git info for sessions: branch, ahead, dirty, worktree; worktree creation (WP7).
//!
//! Everything goes through the `git` CLI (no libgit2). The result type is the protocol's
//! [`GitInfo`], so the daemon can forward it unchanged.

use std::ffi::OsStr;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

mod review;
mod tree;

pub use lotse_proto::{DiffFile, DiffStatus, GitDiff, GitInfo};

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

/// `git -C dir …` with a stable environment: English output, no prompts, and no optional index
/// locks (the status refresh must never race an agent's own `git` calls).
pub(crate) fn command(dir: &Path) -> Command {
    let mut cmd = Command::new("git");
    cmd.arg("-C")
        .arg(dir)
        // A repo's own config must not run commands on the background refresh.
        .args([
            "-c",
            "core.fsmonitor=false",
            "-c",
            "core.untrackedCache=false",
        ])
        .env("LC_ALL", "C")
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("GIT_OPTIONAL_LOCKS", "0")
        .env_remove("GIT_DIR")
        .env_remove("GIT_WORK_TREE")
        .env_remove("GIT_INDEX_FILE");
    cmd
}

fn spawn_error(e: std::io::Error) -> GitError {
    match e.kind() {
        std::io::ErrorKind::NotFound => GitError::GitMissing,
        _ => GitError::Io(e),
    }
}

/// Runs `git -C dir args…` (see [`command`]) and collects its output.
fn run<I, S>(dir: &Path, args: I) -> Result<Output, GitError>
where
    I: IntoIterator<Item = S>,
    S: AsRef<OsStr>,
{
    command(dir).args(args).output().map_err(spawn_error)
}

/// Like [`run`], but reads at most `cap` bytes of stdout: a command that would print more is
/// stopped. Returns the bytes and whether the output was cut. Errors when git fails on its own.
pub(crate) fn run_capped<I, S>(dir: &Path, args: I, cap: usize) -> Result<(Vec<u8>, bool), GitError>
where
    I: IntoIterator<Item = S>,
    S: AsRef<OsStr>,
{
    let mut child = command(dir)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(spawn_error)?;
    let mut out = Vec::new();
    if let Some(stdout) = child.stdout.take() {
        stdout.take(cap as u64 + 1).read_to_end(&mut out)?;
    }
    let cut = out.len() > cap;
    if cut {
        out.truncate(cap);
        let _ = child.kill();
    }
    let status = child.wait()?;
    if !cut && !status.success() {
        return Err(GitError::Command(format!("git exited with {status}")));
    }
    Ok((out, cut))
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
    GitInfo::default()
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
    let status = run(dir, ["status", "--porcelain", "--ignore-submodules=dirty"])?;
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
        // TODO(beta-wp04): behind and +/- from `review`; `pr` is filled by lotsed (opt-in)
        behind: 0,
        added: 0,
        removed: 0,
        pr: None,
    })
}

/// Largest `git diff` text [`diff`] reads (Show Changes).
pub const DIFF_MAX_BYTES: usize = 512 * 1024;
/// Largest diff text kept for one file; the rest of that file is cut.
pub const DIFF_MAX_FILE_BYTES: usize = 128 * 1024;
/// Most untracked file names listed.
const MAX_UNTRACKED: usize = 500;
const EMPTY_TREE: &str = "4b825dc642cb6eb9a060e54bf8d69288fbee4904";

/// Show Changes: what differs in `dir`'s repository from `HEAD` (staged and unstaged together),
/// as one unified diff per file, plus the names of untracked files. Read-only and bounded:
/// [`DIFF_MAX_BYTES`] in total, [`DIFF_MAX_FILE_BYTES`] per file, binary files noted without text.
/// A folder outside any repository yields `GitDiff { is_git: false, .. }`.
///
/// Hardening: the repo's `core.fsmonitor` never runs (see [`command`]), and `--no-ext-diff` /
/// `--no-textconv` keep `diff.external`, `GIT_EXTERNAL_DIFF` and `.gitattributes` textconv drivers
/// from running a program the repo chose.
pub fn diff(dir: &Path) -> Result<GitDiff, GitError> {
    diff_capped(dir, DIFF_MAX_BYTES, DIFF_MAX_FILE_BYTES)
}

/// [`diff`] with explicit caps; tests use small ones.
pub fn diff_capped(dir: &Path, max_total: usize, max_file: usize) -> Result<GitDiff, GitError> {
    if layout(dir)?.is_none() {
        return Ok(GitDiff {
            is_git: false,
            files: Vec::new(),
            truncated: false,
        });
    }
    // a repository without a commit has no HEAD: compare with the empty tree
    let base = if out_ok(dir, ["rev-parse", "--verify", "--quiet", "HEAD"])?.is_some() {
        "HEAD".to_string()
    } else {
        out_ok(dir, ["hash-object", "-t", "tree", "/dev/null"])?
            .filter(|h| !h.is_empty())
            .unwrap_or_else(|| EMPTY_TREE.to_string())
    };
    let (raw, cut) = run_capped(
        dir,
        [
            "-c",
            "core.quotepath=false",
            "diff",
            "--no-color",
            "--no-ext-diff",
            "--no-textconv",
            "--ignore-submodules=dirty",
            "--find-renames",
            "--src-prefix=a/",
            "--dst-prefix=b/",
            base.as_str(),
            "--",
        ],
        max_total,
    )?;
    let text = String::from_utf8_lossy(&raw);
    let mut files: Vec<DiffFile> = split_patch(&text)
        .into_iter()
        .map(|section| parse_section(section, max_file))
        .collect();
    if cut && let Some(last) = files.last_mut() {
        last.truncated = true;
    }
    let mut truncated = cut;

    let (names, names_cut) = run_capped(
        dir,
        [
            "ls-files",
            "--others",
            "--exclude-standard",
            "--full-name",
            "-z",
        ],
        64 * 1024,
    )?;
    let mut names: Vec<&[u8]> = names.split(|b| *b == 0).filter(|n| !n.is_empty()).collect();
    if names_cut {
        names.pop(); // the last name may be cut off
    }
    if names_cut || names.len() > MAX_UNTRACKED {
        truncated = true;
    }
    files.extend(names.into_iter().take(MAX_UNTRACKED).map(|n| DiffFile {
        path: String::from_utf8_lossy(n).into_owned(),
        old_path: None,
        status: DiffStatus::Untracked,
        binary: false,
        truncated: false,
        diff: String::new(),
        added: 0,
        removed: 0,
    }));
    Ok(GitDiff {
        is_git: true,
        files,
        truncated,
    })
}

/// The per-file sections of a `git diff` (each starts at its `diff --git` line).
fn split_patch(text: &str) -> Vec<&str> {
    let mut starts: Vec<usize> = Vec::new();
    let mut at = 0;
    for line in text.split_inclusive('\n') {
        if line.starts_with("diff --git ") {
            starts.push(at);
        }
        at += line.len();
    }
    starts.push(text.len());
    starts.windows(2).map(|w| &text[w[0]..w[1]]).collect()
}

/// `s` cut to at most `max` bytes: on a char boundary, preferably after a whole line.
fn cut_text(s: &str, max: usize) -> &str {
    let mut end = max.min(s.len());
    while !s.is_char_boundary(end) {
        end -= 1;
    }
    let head = &s[..end];
    head.rfind('\n').map_or(head, |i| &head[..=i])
}

fn parse_section(section: &str, max_file: usize) -> DiffFile {
    let (mut status, mut binary) = (DiffStatus::Modified, false);
    let (mut rename_from, mut rename_to, mut minus, mut plus) = (None, None, None, None);
    for line in section.lines().skip(1) {
        if line.starts_with("@@") {
            break; // the headers end at the first hunk
        }
        if let Some(p) = line.strip_prefix("rename from ") {
            rename_from = Some(p.to_string());
        } else if let Some(p) = line.strip_prefix("rename to ") {
            rename_to = Some(p.to_string());
        } else if line.starts_with("new file mode") {
            status = DiffStatus::Added;
        } else if line.starts_with("deleted file mode") {
            status = DiffStatus::Deleted;
        } else if let Some(p) = line.strip_prefix("--- a/") {
            minus = Some(p.trim_end_matches('\t').to_string());
        } else if let Some(p) = line.strip_prefix("+++ b/") {
            plus = Some(p.trim_end_matches('\t').to_string());
        } else if line.starts_with("Binary files ") || line.starts_with("GIT binary patch") {
            binary = true;
        }
    }
    if rename_to.is_some() {
        status = DiffStatus::Renamed;
    }
    // `diff --git a/<path> b/<path>`: both halves are equal unless the file was renamed
    let from_header = || {
        let rest = section.lines().next()?.strip_prefix("diff --git a/")?;
        rest.get(..rest.len().checked_sub(3)? / 2)
            .map(str::to_string)
    };
    let path = rename_to
        .or(plus)
        .or(minus)
        .or_else(from_header)
        .unwrap_or_default();
    let diff = if binary {
        String::new()
    } else {
        cut_text(section, max_file).to_string()
    };
    DiffFile {
        path,
        old_path: rename_from,
        status,
        binary,
        truncated: !binary && diff.len() < section.len(),
        diff,
        // TODO(beta-wp04): count the +/- lines of the section
        added: 0,
        removed: 0,
    }
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
    fn repo_local_fsmonitor_hook_is_never_executed() {
        let d = repo();
        let marker = tempfile::tempdir().unwrap().keep().join("pwned");
        let hook = format!("touch {}; echo", marker.display());
        git(d.path(), &["config", "core.fsmonitor", hook.as_str()]);
        write(d.path(), "b.txt", "new\n");
        let info = git_info(d.path()).unwrap();
        assert_eq!(info.dirty, 1);
        let ran = marker.exists();
        let _ = std::fs::remove_dir_all(marker.parent().unwrap());
        assert!(!ran, "core.fsmonitor command executed");
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

    fn file<'a>(d: &'a GitDiff, path: &str) -> &'a DiffFile {
        d.files
            .iter()
            .find(|f| f.path == path)
            .unwrap_or_else(|| panic!("{path} not in {:?}", d.files))
    }

    #[test]
    fn diff_lists_staged_unstaged_and_untracked_files() {
        let d = repo();
        write(d.path(), "b.txt", "b\n");
        git(d.path(), &["add", "b.txt"]); // staged, new
        write(d.path(), "a.txt", "a\nmore\n"); // unstaged edit
        write(d.path(), "c.txt", "untracked\n");
        let diff = diff(d.path()).unwrap();
        assert!(diff.is_git && !diff.truncated);
        let a = file(&diff, "a.txt");
        assert_eq!(a.status, DiffStatus::Modified);
        assert!(
            a.diff.starts_with("diff --git a/a.txt b/a.txt"),
            "{}",
            a.diff
        );
        assert!(a.diff.contains("\n+more\n"));
        assert_eq!(file(&diff, "b.txt").status, DiffStatus::Added);
        assert!(file(&diff, "b.txt").diff.contains("\n+b\n"));
        let c = file(&diff, "c.txt");
        assert_eq!(c.status, DiffStatus::Untracked);
        assert!(c.diff.is_empty() && !c.binary);
        assert_eq!(diff.files.len(), 3);
    }

    #[test]
    fn diff_of_a_clean_repo_is_empty_and_a_plain_folder_is_no_git() {
        let d = repo();
        let clean = diff(d.path()).unwrap();
        assert!(clean.is_git && clean.files.is_empty());
        let plain = tempfile::tempdir().unwrap();
        let none = diff(plain.path()).unwrap();
        assert!(!none.is_git && none.files.is_empty());
        assert!(matches!(
            diff(&plain.path().join("gone")),
            Err(GitError::NotADirectory(_))
        ));
    }

    #[test]
    fn diff_reports_renames_deletions_and_binaries() {
        let d = repo();
        std::fs::write(d.path().join("img.bin"), [0u8, 159, 146, 150, 0, 1]).unwrap();
        write(d.path(), "gone.txt", "bye\n");
        write(
            d.path(),
            "old name.txt",
            "same content\nline two\nline three\n",
        );
        git(d.path(), &["add", "."]);
        git(d.path(), &["commit", "-q", "-m", "more"]);
        git(d.path(), &["mv", "old name.txt", "new name.txt"]);
        git(d.path(), &["rm", "-q", "gone.txt"]);
        std::fs::write(d.path().join("img.bin"), [0u8, 1, 2, 3, 0, 9]).unwrap();
        let diff = diff(d.path()).unwrap();
        let renamed = file(&diff, "new name.txt");
        assert_eq!(renamed.status, DiffStatus::Renamed);
        assert_eq!(renamed.old_path.as_deref(), Some("old name.txt"));
        assert_eq!(file(&diff, "gone.txt").status, DiffStatus::Deleted);
        let bin = file(&diff, "img.bin");
        assert!(bin.binary && bin.diff.is_empty(), "{bin:?}");
        assert_eq!(bin.status, DiffStatus::Modified);
    }

    #[test]
    fn diff_works_before_the_first_commit() {
        let d = tempfile::tempdir().unwrap();
        git(d.path(), &["init", "-q", "-b", "main"]);
        write(d.path(), "first.txt", "hello\n");
        git(d.path(), &["add", "first.txt"]);
        let diff = diff(d.path()).unwrap();
        assert_eq!(file(&diff, "first.txt").status, DiffStatus::Added);
    }

    #[test]
    fn diff_is_capped_per_file_and_in_total() {
        let d = repo();
        let big = "line of text that is not short\n".repeat(400); // ~12 KB
        for name in ["f1.txt", "f2.txt", "f3.txt"] {
            write(d.path(), name, &big);
            git(d.path(), &["add", name]);
        }
        // per-file cap: every file cut, none dropped
        let per_file = diff_capped(d.path(), 1 << 20, 2_000).unwrap();
        assert_eq!(per_file.files.len(), 3);
        assert!(!per_file.truncated);
        for f in &per_file.files {
            assert!(f.truncated && f.diff.len() <= 2_000, "{}", f.diff.len());
        }
        // total cap: the diff stops, later files are left out
        let total = diff_capped(d.path(), 20_000, 1 << 20).unwrap();
        assert!(total.truncated);
        assert!(total.files.len() < 3, "{}", total.files.len());
        assert!(total.files.last().unwrap().truncated);
        let bytes: usize = total.files.iter().map(|f| f.diff.len()).sum();
        assert!(bytes <= 20_000, "{bytes}");
        // the real caps hold a big change to 512 KB
        write(d.path(), "huge.txt", &big.repeat(100)); // ~1.2 MB
        git(d.path(), &["add", "huge.txt"]);
        let huge = diff(d.path()).unwrap();
        assert!(huge.truncated);
        let bytes: usize = huge.files.iter().map(|f| f.diff.len()).sum();
        assert!(bytes <= DIFF_MAX_BYTES, "{bytes}");
    }

    #[test]
    fn diff_never_runs_commands_from_the_repo_config() {
        let d = repo();
        let dir = tempfile::tempdir().unwrap();
        let marker = |n: &str| dir.path().join(n);
        let touch = |n: &str| format!("touch {}; echo", marker(n).display());
        git(d.path(), &["config", "core.fsmonitor", &touch("fsmonitor")]);
        git(d.path(), &["config", "diff.external", &touch("external")]);
        git(d.path(), &["config", "diff.t.textconv", &touch("textconv")]);
        write(d.path(), ".gitattributes", "*.txt diff=t\n");
        write(d.path(), "a.txt", "changed\n");
        let diff = diff(d.path()).unwrap();
        assert_eq!(file(&diff, "a.txt").status, DiffStatus::Modified);
        for n in ["fsmonitor", "external", "textconv"] {
            assert!(!marker(n).exists(), "core/diff {n} command executed");
        }
    }
}
