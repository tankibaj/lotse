//! Beta: the file tree of a session's folder with git status, and file content at a revision (wp05).
//!
//! Read-only. Confinement of the paths a client asks for is the daemon's job (`lotsed::files`);
//! what git is given here never starts with `-`, and everything it prints is size-capped.

use super::{EMPTY_TREE, GitError, layout, out_ok, run_capped};
use lotse_proto::{DiffStatus, FileEntry, FileTree};
use std::collections::BTreeMap;
use std::path::Path;

/// Most files listed; more sets `FileTree::truncated`.
pub const MAX_ENTRIES: usize = 20_000;
/// Most bytes read from one listing command.
const MAX_LIST_BYTES: usize = 16 * 1024 * 1024;
const MAX_REV_LEN: usize = 256;

/// The files of `dir` (git: tracked and untracked, ignored left out, each with its status against
/// `HEAD` and +/- counts; `changed_only`: just the changed ones), with the folders above them.
/// A folder outside any repository is walked instead (dot folders skipped, symlinks not followed),
/// and has no changes. Paths are relative to `dir` and `/` separated, sorted.
pub fn list_files(dir: &Path, changed_only: bool) -> Result<FileTree, GitError> {
    list_capped(dir, changed_only, MAX_ENTRIES)
}

fn list_capped(dir: &Path, changed_only: bool, cap: usize) -> Result<FileTree, GitError> {
    if !dir.is_dir() {
        return Err(GitError::NotADirectory(dir.to_path_buf()));
    }
    let root = dir.canonicalize()?;
    let is_git = layout(&root)?.is_some();
    let (entries, truncated) = match (is_git, changed_only) {
        (true, _) => git_files(&root, changed_only, cap)?,
        (false, true) => (Vec::new(), false),
        (false, false) => walk(&root, cap),
    };
    Ok(FileTree {
        root: root.to_string_lossy().into_owned(),
        is_git,
        entries,
        truncated,
    })
}

/// The files collected so far; one entry per path, at most `cap` of them.
struct Files {
    by_path: BTreeMap<String, FileEntry>,
    cap: usize,
    truncated: bool,
}

impl Files {
    /// The entry of `path`, made when new; `None` (and `truncated`) when the cap is reached.
    fn entry(&mut self, path: &str, dir: bool) -> Option<&mut FileEntry> {
        if !self.by_path.contains_key(path) {
            if self.by_path.len() >= self.cap {
                self.truncated = true;
                return None;
            }
            self.by_path.insert(
                path.to_owned(),
                FileEntry {
                    path: path.to_owned(),
                    dir,
                    status: None,
                    added: 0,
                    removed: 0,
                },
            );
        }
        self.by_path.get_mut(path)
    }

    /// The entries with the folders above every file, sorted by path.
    fn finish(self) -> (Vec<FileEntry>, bool) {
        let mut all = self.by_path;
        let parents: Vec<String> = all
            .keys()
            .flat_map(|p| p.match_indices('/').map(|(i, _)| p[..i].to_owned()))
            .collect();
        for dir in parents {
            all.entry(dir.clone()).or_insert(FileEntry {
                path: dir,
                dir: true,
                status: None,
                added: 0,
                removed: 0,
            });
        }
        (all.into_values().collect(), self.truncated)
    }
}

/// The NUL-terminated items of `-z` output; what follows the last NUL is empty, or an item the
/// byte cap cut off: dropped.
fn items(out: &[u8]) -> Vec<&str> {
    let mut parts: Vec<&[u8]> = out.split(|b| *b == 0).collect();
    parts.pop();
    // a path that is not UTF-8 cannot be sent back and read: left out
    parts
        .into_iter()
        .filter_map(|p| std::str::from_utf8(p).ok())
        .collect()
}

fn git_files(
    root: &Path,
    changed_only: bool,
    cap: usize,
) -> Result<(Vec<FileEntry>, bool), GitError> {
    // an unborn branch has no HEAD: everything staged counts as added
    let base = if out_ok(root, ["rev-parse", "--verify", "-q", "HEAD"])?.is_some() {
        "HEAD"
    } else {
        EMPTY_TREE
    };
    let diff = |mode: &str| {
        run_capped(
            root,
            [
                "diff",
                mode,
                "-z",
                "--no-renames",
                "--no-ext-diff",
                "--no-textconv",
                "--ignore-submodules=dirty",
                "--relative",
                base,
            ],
            MAX_LIST_BYTES,
        )
    };
    let mut files = Files {
        by_path: BTreeMap::new(),
        cap,
        truncated: false,
    };

    // changed files first: they are never the ones a cap leaves out
    let (out, cut) = diff("--name-status")?;
    files.truncated |= cut;
    for pair in items(&out).chunks_exact(2) {
        let status = match pair[0] {
            "A" => DiffStatus::Added,
            "D" => DiffStatus::Deleted,
            _ => DiffStatus::Modified,
        };
        if let Some(e) = files.entry(pair[1], false) {
            e.status = Some(status);
        }
    }
    let (out, cut) = diff("--numstat")?;
    files.truncated |= cut;
    for line in items(&out) {
        let mut cols = line.splitn(3, '\t');
        let (Some(added), Some(removed), Some(path)) = (cols.next(), cols.next(), cols.next())
        else {
            continue;
        };
        // a binary file counts "-"
        if let Some(e) = files.by_path.get_mut(path) {
            e.added = added.parse().unwrap_or(0);
            e.removed = removed.parse().unwrap_or(0);
        }
    }

    let (out, cut) = run_capped(
        root,
        ["ls-files", "--others", "--exclude-standard", "-z"],
        MAX_LIST_BYTES,
    )?;
    files.truncated |= cut;
    for path in items(&out) {
        // a nested repository is listed as `dir/`, not looked into
        let (name, dir) = path.strip_suffix('/').map_or((path, false), |n| (n, true));
        if let Some(e) = files.entry(name, dir) {
            e.status = (!dir).then_some(DiffStatus::Untracked);
        }
    }

    if !changed_only {
        let (out, cut) = run_capped(root, ["ls-files", "--cached", "-z"], MAX_LIST_BYTES)?;
        files.truncated |= cut;
        for path in items(&out) {
            files.entry(path, false);
        }
    }
    Ok(files.finish())
}

/// Depth-first walk of a folder that is no repository: files and folders, no dot entries, no
/// symlink followed (a link is listed as a file).
fn walk(root: &Path, cap: usize) -> (Vec<FileEntry>, bool) {
    let mut files = Files {
        by_path: BTreeMap::new(),
        cap,
        truncated: false,
    };
    let mut todo = vec![(root.to_path_buf(), String::new())];
    while let Some((dir, prefix)) = todo.pop() {
        let Ok(read) = std::fs::read_dir(&dir) else {
            continue; // unreadable: left out
        };
        let mut children: Vec<_> = read.filter_map(Result::ok).collect();
        children.sort_by_key(|c| c.file_name());
        for child in children {
            let Ok(name) = child.file_name().into_string() else {
                continue;
            };
            if name.starts_with('.') {
                continue;
            }
            let rel = format!("{prefix}{name}");
            let is_dir = child.file_type().is_ok_and(|t| t.is_dir());
            if files.entry(&rel, is_dir).is_none() {
                return files.finish();
            }
            if is_dir {
                todo.push((child.path(), format!("{rel}/")));
            }
        }
    }
    files.finish()
}

/// A revision git may be given as one argument: no option (`-…`), no whitespace or control
/// characters.
pub fn valid_rev(rev: &str) -> bool {
    !rev.is_empty()
        && rev.len() <= MAX_REV_LEN
        && !rev.starts_with('-')
        && !rev.chars().any(|c| c.is_control() || c.is_whitespace())
}

/// A file's bytes at a revision.
#[derive(Debug, PartialEq, Eq)]
pub struct Blob {
    /// At most the cap asked for.
    pub bytes: Vec<u8>,
    /// The file's real size.
    pub size: u64,
    /// `size` is over the cap: `bytes` is its start.
    pub cut: bool,
}

/// `rel` (relative to `dir`, clean: no `..`, not absolute) as it was at `rev`, without
/// filters or textconv. An unknown revision, a missing path or a folder is `GitError::Command`.
pub fn read_blob(dir: &Path, rev: &str, rel: &str, cap: usize) -> Result<Blob, GitError> {
    let clean = !rel.is_empty()
        && !rel.starts_with('/')
        && !rel.contains('\0')
        && !rel.split('/').any(|part| part == "..");
    if !valid_rev(rev) || !clean {
        return Err(GitError::Command("invalid revision or path".into()));
    }
    // `rev:./path` is relative to `dir`, not to the repository root
    let spec = format!("{rev}:./{rel}");
    let (bytes, cut) = run_capped(dir, ["cat-file", "blob", spec.as_str()], cap)?;
    let size = if cut {
        out_ok(dir, ["cat-file", "-s", spec.as_str()])?
            .and_then(|s| s.parse().ok())
            .unwrap_or(cap as u64 + 1)
    } else {
        bytes.len() as u64
    };
    Ok(Blob { bytes, size, cut })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;
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

    fn put(dir: &Path, rel: &str, text: &str) {
        let p = dir.join(rel);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, text).unwrap();
    }

    fn repo() -> tempfile::TempDir {
        let tmp = tempfile::tempdir().unwrap();
        git(tmp.path(), &["init", "-q", "-b", "main"]);
        tmp
    }

    fn paths(t: &FileTree) -> Vec<&str> {
        t.entries.iter().map(|e| e.path.as_str()).collect()
    }

    fn real(tmp: &tempfile::TempDir) -> PathBuf {
        tmp.path().canonicalize().unwrap()
    }

    #[test]
    fn items_drop_what_the_cap_cut() {
        assert_eq!(items(b"a\0b\0"), ["a", "b"]);
        assert_eq!(items(b"a\0bc"), ["a"]);
        assert!(items(b"").is_empty());
        // not UTF-8: left out
        assert_eq!(items(b"a\0\xff\0b\0"), ["a", "b"]);
    }

    #[test]
    fn revs_that_could_be_options_or_carry_whitespace_are_invalid() {
        for ok in ["HEAD", "main", "origin/main", "HEAD~2", "abc123", "v1.0^{}"] {
            assert!(valid_rev(ok), "{ok}");
        }
        for bad in [
            "",
            "-p",
            "--output=x",
            "--",
            "a b",
            "a\nb",
            "a\0b",
            "\tHEAD",
            &"a".repeat(257),
        ] {
            assert!(!valid_rev(bad), "{bad:?}");
        }
    }

    #[test]
    fn read_blob_refuses_what_validation_would() {
        let tmp = repo();
        for (rev, rel) in [
            ("-p", "a"),
            ("HEAD", "../a"),
            ("HEAD", "/etc/passwd"),
            ("HEAD", ""),
            ("HEAD", "a/../../b"),
            ("HEAD", "a\0b"),
        ] {
            assert!(
                matches!(
                    read_blob(tmp.path(), rev, rel, 10),
                    Err(GitError::Command(_))
                ),
                "{rev} {rel:?}"
            );
        }
    }

    #[test]
    fn read_blob_reports_the_real_size_of_a_cut_file() {
        let tmp = repo();
        put(tmp.path(), "big.txt", "0123456789");
        git(tmp.path(), &["add", "."]);
        git(tmp.path(), &["commit", "-q", "-m", "x"]);
        let b = read_blob(tmp.path(), "HEAD", "big.txt", 4).unwrap();
        assert_eq!(
            b,
            Blob {
                bytes: b"0123".to_vec(),
                size: 10,
                cut: true
            }
        );
        let whole = read_blob(tmp.path(), "HEAD", "big.txt", 10).unwrap();
        assert_eq!((whole.size, whole.cut), (10, false));
        assert!(read_blob(tmp.path(), "HEAD", "nope.txt", 10).is_err());
        assert!(read_blob(tmp.path(), "no-such-rev", "big.txt", 10).is_err());
    }

    #[test]
    fn walk_skips_dot_entries_does_not_follow_links_and_stops_at_the_cap() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path();
        put(dir, "a/1.txt", "");
        put(dir, "a/2.txt", "");
        put(dir, "b.txt", "");
        put(dir, ".hid/x", "");
        put(dir, ".dot", "");
        std::os::unix::fs::symlink(dir.join("a"), dir.join("link")).unwrap();

        let all = list_capped(dir, false, 100).unwrap();
        assert_eq!(paths(&all), ["a", "a/1.txt", "a/2.txt", "b.txt", "link"]);
        assert!(!all.entries.last().unwrap().dir, "a link is a file");
        assert!(!all.truncated && !all.is_git);

        let cut = list_capped(dir, false, 3).unwrap();
        assert!(cut.truncated);
        assert_eq!(cut.entries.len(), 3);
        assert!(list_capped(dir, true, 3).unwrap().entries.is_empty());
    }

    #[test]
    fn git_listing_puts_changed_files_before_the_cap_cuts() {
        let tmp = repo();
        for n in 0..5 {
            put(tmp.path(), &format!("f{n}.txt"), "x\n");
        }
        git(tmp.path(), &["add", "."]);
        git(tmp.path(), &["commit", "-q", "-m", "x"]);
        put(tmp.path(), "f4.txt", "y\n");
        put(tmp.path(), "zz-new.txt", "n\n");

        let t = list_capped(tmp.path(), false, 3).unwrap();
        assert!(t.truncated);
        let names = paths(&t);
        assert!(
            names.contains(&"f4.txt") && names.contains(&"zz-new.txt"),
            "{names:?}"
        );
        assert_eq!(names.len(), 3);
        assert!(!list_capped(tmp.path(), true, 3).unwrap().truncated);
    }

    #[test]
    fn a_fresh_repo_without_commits_lists_staged_and_untracked_files() {
        let tmp = repo();
        put(tmp.path(), "staged.txt", "a\nb\n");
        put(tmp.path(), "loose.txt", "c\n");
        git(tmp.path(), &["add", "staged.txt"]);

        let t = list_files(tmp.path(), false).unwrap();
        let by = |p: &str| t.entries.iter().find(|e| e.path == p).unwrap();
        assert_eq!(
            (by("staged.txt").status, by("staged.txt").added),
            (Some(DiffStatus::Added), 2)
        );
        assert_eq!(by("loose.txt").status, Some(DiffStatus::Untracked));
        assert_eq!(t.root, real(&tmp).to_string_lossy());
    }

    #[test]
    fn a_binary_change_counts_zero_and_a_nested_repo_is_a_folder() {
        let tmp = repo();
        std::fs::write(tmp.path().join("img.bin"), [0u8, 1, 2]).unwrap();
        git(tmp.path(), &["add", "."]);
        git(tmp.path(), &["commit", "-q", "-m", "x"]);
        std::fs::write(tmp.path().join("img.bin"), [0u8, 9, 9, 9]).unwrap();
        let inner = tmp.path().join("vendor/inner");
        std::fs::create_dir_all(&inner).unwrap();
        git(&inner, &["init", "-q", "-b", "main"]);
        put(&inner, "f", "x");

        let t = list_files(tmp.path(), false).unwrap();
        let by = |p: &str| t.entries.iter().find(|e| e.path == p).unwrap();
        assert_eq!(
            (
                by("img.bin").status,
                by("img.bin").added,
                by("img.bin").removed
            ),
            (Some(DiffStatus::Modified), 0, 0)
        );
        assert!(by("vendor/inner").dir);
        assert!(!paths(&t).contains(&"vendor/inner/f"));
    }

    #[test]
    fn not_a_folder_is_an_error() {
        assert!(matches!(
            list_files(Path::new("/definitely/not/here"), false),
            Err(GitError::NotADirectory(_))
        ));
    }
}
