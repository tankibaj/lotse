//! Beta: branch against its base (ahead, behind, commits), +/- counts, the merge-base diff (wp04).

//!
//! The base is the default branch: `origin/HEAD`, else `main`, else `master`. Everything runs
//! through the hardened `command()` (no repo-chosen programs) and is read-only.

use super::{
    DIFF_MAX_BYTES, DIFF_MAX_FILE_BYTES, GitDiff, GitError, diff, diff_against, head_or_empty_tree,
    layout, out_ok, run,
};
use lotse_proto::{Review, ReviewCommit};
use std::path::Path;

/// Most commits `Review::commits` lists.
pub const MAX_COMMITS: usize = 50;

/// Does `refname` (a full ref, never user input) name a commit?
fn resolves(dir: &Path, refname: &str) -> Result<bool, GitError> {
    let spec = format!("{refname}^{{commit}}");
    Ok(out_ok(dir, ["rev-parse", "--verify", "--quiet", spec.as_str()])?.is_some())
}

/// The ref the branch is compared with: `origin/HEAD`'s target (`origin/main`), else a local
/// `main`, else `master`; `None` when the repository has none of them.
pub fn default_branch(dir: &Path) -> Result<Option<String>, GitError> {
    if resolves(dir, "refs/remotes/origin/HEAD")?
        && let Some(short) = out_ok(
            dir,
            [
                "symbolic-ref",
                "--quiet",
                "--short",
                "refs/remotes/origin/HEAD",
            ],
        )?
        .filter(|s| !s.is_empty())
    {
        return Ok(Some(short));
    }
    for name in ["main", "master"] {
        if resolves(dir, &format!("refs/heads/{name}"))? {
            return Ok(Some(name.to_string()));
        }
    }
    Ok(None)
}

/// `(ahead, behind)` of `HEAD` against `base`; `(0, 0)` when git cannot compare them (no commit
/// yet, unknown ref).
pub fn ahead_behind(dir: &Path, base: &str) -> Result<(u32, u32), GitError> {
    let range = format!("{base}...HEAD");
    let counts = out_ok(
        dir,
        ["rev-list", "--left-right", "--count", range.as_str(), "--"],
    )?;
    // `<left>\t<right>`: left is what only the base has, right what only HEAD has
    let parsed = counts.and_then(|c| {
        let (left, right) = c.split_once('\t')?;
        Some((right.trim().parse().ok()?, left.trim().parse().ok()?))
    });
    Ok(parsed.unwrap_or((0, 0)))
}

/// The newest `cap` commits of `base..HEAD` and whether there are more.
pub fn commits_ahead(
    dir: &Path,
    base: &str,
    cap: usize,
) -> Result<(Vec<ReviewCommit>, bool), GitError> {
    let range = format!("{base}..HEAD");
    let limit = format!("-n{}", cap + 1);
    // `-z` ends each record with NUL and the format separates the fields with NUL: no subject
    // or name can break the split
    let o = run(
        dir,
        [
            "log",
            "-z",
            "--no-color",
            "--no-show-signature",
            "--format=%H%x00%an%x00%at%x00%s",
            limit.as_str(),
            range.as_str(),
            "--",
        ],
    )?;
    if !o.status.success() {
        return Ok((Vec::new(), false));
    }
    let text = String::from_utf8_lossy(&o.stdout);
    let fields: Vec<&str> = text.split('\0').filter(|f| !f.is_empty()).collect();
    let mut commits: Vec<ReviewCommit> = fields
        .chunks_exact(4)
        .map(|f| ReviewCommit {
            sha: f[0].to_string(),
            author: f[1].to_string(),
            at: f[2].parse::<u64>().unwrap_or(0).saturating_mul(1000),
            subject: f[3].to_string(),
        })
        .collect();
    let more = commits.len() > cap;
    commits.truncate(cap);
    Ok((commits, more))
}

/// Lines added and removed by the uncommitted changes (staged and unstaged, tracked files);
/// binary files count 0.
pub fn numstat(dir: &Path) -> Result<(u32, u32), GitError> {
    let base = head_or_empty_tree(dir)?;
    let o = run(
        dir,
        [
            "diff",
            "--numstat",
            "--no-ext-diff",
            "--no-textconv",
            "--ignore-submodules=dirty",
            base.as_str(),
            "--",
        ],
    )?;
    if !o.status.success() {
        return Ok((0, 0));
    }
    let (mut added, mut removed) = (0u32, 0u32);
    for line in String::from_utf8_lossy(&o.stdout).lines() {
        let mut cols = line.split('\t');
        // binary files print `-` for both
        if let (Some(a), Some(r)) = (cols.next(), cols.next())
            && let (Ok(a), Ok(r)) = (a.parse::<u32>(), r.parse::<u32>())
        {
            added = added.saturating_add(a);
            removed = removed.saturating_add(r);
        }
    }
    Ok((added, removed))
}

/// How the branch of `dir` stands against the default branch. A folder outside a repository, or a
/// repository without `origin/HEAD`, `main` and `master`, yields an empty base and zero counts.
pub fn review(dir: &Path) -> Result<Review, GitError> {
    let mut r = Review {
        base: String::new(),
        ahead: 0,
        behind: 0,
        commits: Vec::new(),
        truncated: false,
    };
    if layout(dir)?.is_none() {
        return Ok(r);
    }
    let Some(base) = default_branch(dir)? else {
        return Ok(r);
    };
    (r.ahead, r.behind) = ahead_behind(dir, &base)?;
    (r.commits, r.truncated) = commits_ahead(dir, &base, MAX_COMMITS)?;
    r.base = base;
    Ok(r)
}

/// Everything the branch adds: the working tree against the merge-base with the default branch
/// (commits and uncommitted changes), plus untracked names. Without a default branch, or without a
/// common ancestor, this is the diff from `HEAD`.
pub fn diff_from_merge_base(dir: &Path) -> Result<GitDiff, GitError> {
    if layout(dir)?.is_none() {
        return diff(dir);
    }
    let Some(base) = default_branch(dir)? else {
        return diff(dir);
    };
    let merge_base = out_ok(dir, ["merge-base", base.as_str(), "HEAD"])?.filter(|s| !s.is_empty());
    match merge_base {
        Some(sha) => diff_against(dir, &sha, DIFF_MAX_BYTES, DIFF_MAX_FILE_BYTES),
        None => diff(dir),
    }
}

/// Host of the `origin` remote (lower case, port and user left out), for choosing `gh` or `glab`.
/// `None` without an origin, or when the host would read as an option or has odd characters.
pub fn origin_host(dir: &Path) -> Option<String> {
    let url = out_ok(dir, ["remote", "get-url", "origin"]).ok()??;
    let host = match url.split_once("://") {
        Some((_, rest)) => {
            let authority = rest.split('/').next().unwrap_or("");
            let host = authority.rsplit('@').next().unwrap_or("");
            host.split(':').next().unwrap_or("")
        }
        // scp-like: `[user@]host:path`
        None => {
            let rest = url.rsplit_once('@').map_or(url.as_str(), |(_, r)| r);
            rest.split(':').next().unwrap_or("")
        }
    };
    let host = host.to_ascii_lowercase();
    let ok = !host.is_empty()
        && host.starts_with(|c: char| c.is_ascii_alphanumeric())
        && host
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-'));
    ok.then_some(host)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{DiffStatus, git_info};
    use std::process::Command;
    use tempfile::TempDir;

    fn git(dir: &Path, args: &[&str]) {
        let o = Command::new("git")
            .arg("-C")
            .arg(dir)
            .args(["-c", "commit.gpgsign=false"])
            .args(args)
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_SYSTEM", "/dev/null")
            .env("GIT_AUTHOR_NAME", "Ada")
            .env("GIT_AUTHOR_EMAIL", "t@example.com")
            .env("GIT_COMMITTER_NAME", "Ada")
            .env("GIT_COMMITTER_EMAIL", "t@example.com")
            .output()
            .expect("git runs");
        assert!(
            o.status.success(),
            "git {args:?}: {}",
            String::from_utf8_lossy(&o.stderr)
        );
    }

    fn write(dir: &Path, name: &str, text: &str) {
        std::fs::write(dir.join(name), text).unwrap();
    }

    fn commit(dir: &Path, name: &str, text: &str, msg: &str) {
        write(dir, name, text);
        git(dir, &["add", "."]);
        git(dir, &["commit", "-q", "-m", msg]);
    }

    /// `main` with one commit.
    fn repo() -> TempDir {
        let d = tempfile::tempdir().unwrap();
        git(d.path(), &["init", "-q", "-b", "main"]);
        commit(d.path(), "a.txt", "a\n", "init");
        d
    }

    /// `main` has one commit the branch lacks; `feature` has two main lacks.
    fn diverged() -> TempDir {
        let d = repo();
        git(d.path(), &["checkout", "-q", "-b", "feature"]);
        commit(d.path(), "f1.txt", "one\n", "add f1");
        commit(d.path(), "f2.txt", "two\ntwo\n", "add f2");
        git(d.path(), &["checkout", "-q", "main"]);
        commit(d.path(), "m.txt", "m\n", "main moves");
        git(d.path(), &["checkout", "-q", "feature"]);
        d
    }

    #[test]
    fn default_branch_prefers_origin_head_then_main_then_master() {
        let d = repo();
        assert_eq!(default_branch(d.path()).unwrap().as_deref(), Some("main"));

        let m = tempfile::tempdir().unwrap();
        git(m.path(), &["init", "-q", "-b", "master"]);
        commit(m.path(), "a.txt", "a\n", "init");
        assert_eq!(default_branch(m.path()).unwrap().as_deref(), Some("master"));

        // origin/HEAD wins over a local main
        let origin = tempfile::tempdir().unwrap();
        git(origin.path(), &["init", "-q", "-b", "trunk"]);
        commit(origin.path(), "a.txt", "a\n", "init");
        let c = tempfile::tempdir().unwrap();
        let url = origin.path().to_str().unwrap();
        git(c.path(), &["clone", "-q", url, "."]);
        git(c.path(), &["branch", "main"]);
        assert_eq!(
            default_branch(c.path()).unwrap().as_deref(),
            Some("origin/trunk")
        );
    }

    #[test]
    fn default_branch_is_none_without_main_master_or_origin_head() {
        let d = tempfile::tempdir().unwrap();
        git(d.path(), &["init", "-q", "-b", "trunk"]);
        commit(d.path(), "a.txt", "a\n", "init");
        assert_eq!(default_branch(d.path()).unwrap(), None);
        let r = review(d.path()).unwrap();
        assert_eq!(
            (
                r.base.as_str(),
                r.ahead,
                r.behind,
                r.commits.len(),
                r.truncated
            ),
            ("", 0, 0, 0, false)
        );
    }

    #[test]
    fn ahead_and_behind_count_each_side_of_the_base() {
        let d = diverged();
        assert_eq!(ahead_behind(d.path(), "main").unwrap(), (2, 1));
        // an unborn HEAD or an unknown ref is 0/0, not an error
        let fresh = tempfile::tempdir().unwrap();
        git(fresh.path(), &["init", "-q", "-b", "main"]);
        assert_eq!(ahead_behind(fresh.path(), "main").unwrap(), (0, 0));
    }

    #[test]
    fn commits_ahead_lists_newest_first_with_author_and_time() {
        let d = diverged();
        let (commits, cut) = commits_ahead(d.path(), "main", MAX_COMMITS).unwrap();
        assert!(!cut);
        let subjects: Vec<_> = commits.iter().map(|c| c.subject.as_str()).collect();
        assert_eq!(subjects, ["add f2", "add f1"]);
        assert_eq!(commits[0].author, "Ada");
        assert_eq!(commits[0].sha.len(), 40);
        assert!(commits[0].at > 1_600_000_000_000, "{}", commits[0].at);
    }

    #[test]
    fn commits_ahead_is_capped_and_keeps_odd_subjects_whole() {
        let d = repo();
        git(d.path(), &["checkout", "-q", "-b", "many"]);
        for n in 0..5 {
            commit(
                d.path(),
                "a.txt",
                &format!("{n}\n"),
                &format!("c{n} | \"q\" é"),
            );
        }
        let (commits, cut) = commits_ahead(d.path(), "main", 3).unwrap();
        assert!(cut);
        assert_eq!(commits.len(), 3);
        assert_eq!(commits[0].subject, "c4 | \"q\" é");
        let (all, cut) = commits_ahead(d.path(), "main", 5).unwrap();
        assert!(!cut && all.len() == 5);
    }

    #[test]
    fn review_reports_base_ahead_behind_and_commits() {
        let d = diverged();
        let r = review(d.path()).unwrap();
        assert_eq!(r.base, "main");
        assert_eq!((r.ahead, r.behind), (2, 1));
        assert_eq!(r.commits.len(), 2);
        let plain = tempfile::tempdir().unwrap();
        let none = review(plain.path()).unwrap();
        assert_eq!(
            (none.base.as_str(), none.ahead, none.commits.len()),
            ("", 0, 0)
        );
    }

    #[test]
    fn numstat_sums_uncommitted_lines_and_skips_binaries() {
        let d = repo();
        write(d.path(), "a.txt", "a\nb\nc\n"); // +2
        write(d.path(), "b.txt", "x\ny\n");
        git(d.path(), &["add", "b.txt"]); // +2 staged
        std::fs::write(d.path().join("img.bin"), [0u8, 1, 2, 0]).unwrap();
        git(d.path(), &["add", "img.bin"]);
        write(d.path(), "untracked.txt", "u\n"); // not counted
        assert_eq!(numstat(d.path()).unwrap(), (4, 0));
        git(d.path(), &["commit", "-q", "-am", "more"]);
        write(d.path(), "a.txt", "c\n"); // `a\nb\nc\n` -> `c\n`: -2
        assert_eq!(numstat(d.path()).unwrap(), (0, 2));
    }

    #[test]
    fn git_info_fills_behind_and_the_uncommitted_counts() {
        let d = diverged();
        write(d.path(), "f1.txt", "one\nmore\n");
        let info = git_info(d.path()).unwrap();
        assert_eq!((info.behind, info.added, info.removed), (1, 1, 0));
        assert_eq!(info.pr, None);
    }

    #[test]
    fn diff_from_merge_base_counts_lines() {
        let d = diverged();
        write(d.path(), "f2.txt", "two\ntwo\nthree\n"); // uncommitted: +1
        write(d.path(), "u.txt", "untracked\n");
        let diff = diff_from_merge_base(d.path()).unwrap();
        assert!(diff.is_git && !diff.truncated);
        let get = |p: &str| diff.files.iter().find(|f| f.path == p).unwrap();
        // committed on the branch since the merge-base
        assert_eq!((get("f1.txt").added, get("f1.txt").removed), (1, 0));
        assert_eq!(get("f1.txt").status, DiffStatus::Added);
        assert_eq!((get("f2.txt").added, get("f2.txt").removed), (3, 0));
        // main's own commit is not part of the branch's change
        assert!(diff.files.iter().all(|f| f.path != "m.txt"));
        assert_eq!(get("u.txt").status, DiffStatus::Untracked);
        assert!(get("f1.txt").diff.contains("\n+one\n"));
    }

    #[test]
    fn diff_counts_removed_lines_and_ignores_header_lines() {
        let d = repo();
        commit(d.path(), "a.txt", "1\n2\n3\n", "three");
        write(d.path(), "a.txt", "1\n--x\n"); // -2 (`2`, `3`), +1 (`--x`)
        let diff = crate::diff(d.path()).unwrap();
        let a = diff.files.iter().find(|f| f.path == "a.txt").unwrap();
        assert_eq!((a.added, a.removed), (1, 2));
    }

    #[test]
    fn diff_from_merge_base_without_a_base_is_the_diff_from_head() {
        let d = tempfile::tempdir().unwrap();
        git(d.path(), &["init", "-q", "-b", "trunk"]);
        commit(d.path(), "a.txt", "a\n", "init");
        write(d.path(), "a.txt", "a\nb\n");
        let diff = diff_from_merge_base(d.path()).unwrap();
        assert_eq!(diff.files.len(), 1);
        assert_eq!(diff.files[0].added, 1);
        let plain = tempfile::tempdir().unwrap();
        assert!(!diff_from_merge_base(plain.path()).unwrap().is_git);
    }

    #[test]
    fn origin_host_reads_url_forms() {
        let d = repo();
        assert_eq!(origin_host(d.path()), None);
        for (url, host) in [
            ("https://github.com/o/r.git", "github.com"),
            ("https://user:pw@git.cgm.ag:8443/o/r.git", "git.cgm.ag"),
            (
                "ssh://git@GitLab.Example.com:2222/o/r.git",
                "gitlab.example.com",
            ),
            ("git@github.com:o/r.git", "github.com"),
        ] {
            git(d.path(), &["config", "remote.origin.url", url]);
            assert_eq!(origin_host(d.path()).as_deref(), Some(host), "{url}");
        }
        // a host that reads as an option never reaches `gh auth status --hostname`
        git(d.path(), &["config", "remote.origin.url", "-oProxy=x:y/z"]);
        assert_eq!(origin_host(d.path()), None);
    }
}
