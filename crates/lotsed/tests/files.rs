//! Beta wp05 on the real daemon: `list_files`, `read_file`, `write_file`, confined to the session
//! folder. Temp data dir, temp HOME, temp project folders: nothing of the real `~/.claude` or Lotse
//! data is touched, and no agent runs (the sessions are plain shells that stay idle).

mod common;

use common::{Client, Daemon};
use lotse_proto::{DiffStatus, ErrorCode, File, FileEntry, FileTree, Request, Response, Session};
use std::path::{Path, PathBuf};
use std::process::Command;

/// SHA-256 of `hello\n`.
const HELLO_SHA: &str = "5891b5b522d5df086d0ff0b110fbd9d21bb4fc7163af34d08286a2e846f6be03";
const MAX_BYTES: usize = 2 * 1024 * 1024;

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

/// A committed repo: `a.txt` (hello), `sub/b.txt`, an ignored `ignored.log`.
fn repo(d: &Daemon, name: &str) -> PathBuf {
    let dir = d.project_dir(name);
    git(&dir, &["init", "-q", "-b", "main"]);
    put(&dir, ".gitignore", "*.log\n");
    put(&dir, "a.txt", "hello\n");
    put(&dir, "sub/b.txt", "bee\n");
    git(&dir, &["add", "."]);
    git(&dir, &["commit", "-q", "-m", "init"]);
    put(&dir, "ignored.log", "noise\n");
    dir
}

fn open(c: &mut Client, dir: &Path) -> Session {
    let project = c.add_project(dir);
    c.create_session(&project, Some("/bin/sh"))
}

fn list(c: &mut Client, s: &Session, changed_only: bool) -> Response {
    c.request(Request::ListFiles {
        session_id: s.id.clone(),
        changed_only,
    })
}

fn read(c: &mut Client, s: &Session, path: &str, rev: Option<&str>) -> Response {
    c.request(Request::ReadFile {
        session_id: s.id.clone(),
        path: path.into(),
        rev: rev.map(str::to_owned),
    })
}

fn write(c: &mut Client, s: &Session, path: &str, text: &str, sha: Option<&str>) -> Response {
    c.request(Request::WriteFile {
        session_id: s.id.clone(),
        path: path.into(),
        text: text.into(),
        expected_sha: sha.map(str::to_owned),
    })
}

fn tree(r: Response) -> FileTree {
    match r {
        Response::FileTree(t) => t,
        other => panic!("expected file_tree, got {other:?}"),
    }
}

fn file(r: Response) -> File {
    match r {
        Response::File(f) => f,
        other => panic!("expected file, got {other:?}"),
    }
}

fn is_err(r: &Response, code: ErrorCode) -> bool {
    matches!(r, Response::Error(e) if e.code == code)
}

fn paths(t: &FileTree) -> Vec<&str> {
    t.entries.iter().map(|e| e.path.as_str()).collect()
}

fn entry<'a>(t: &'a FileTree, path: &str) -> &'a FileEntry {
    t.entries
        .iter()
        .find(|e| e.path == path)
        .unwrap_or_else(|| panic!("{path} not in {:?}", paths(t)))
}

#[test]
fn list_files_of_a_repo_shows_tracked_and_untracked_with_status_and_skips_ignored() {
    let d = Daemon::start();
    let mut c = d.client();
    let dir = repo(&d, "proj");
    put(&dir, "a.txt", "hello\nmore\n"); // modified: +1
    put(&dir, "new.txt", "x\n"); // untracked
    let s = open(&mut c, &dir);

    let t = tree(list(&mut c, &s, false));
    assert!(t.is_git && !t.truncated);
    assert_eq!(
        Path::new(&t.root).canonicalize().unwrap(),
        dir.canonicalize().unwrap()
    );
    assert_eq!(
        paths(&t),
        [".gitignore", "a.txt", "new.txt", "sub", "sub/b.txt"]
    );
    assert!(entry(&t, "sub").dir && !entry(&t, "sub/b.txt").dir);
    let a = entry(&t, "a.txt");
    assert_eq!(
        (a.status, a.added, a.removed),
        (Some(DiffStatus::Modified), 1, 0)
    );
    assert_eq!(entry(&t, "new.txt").status, Some(DiffStatus::Untracked));
    assert_eq!(entry(&t, "sub/b.txt").status, None);
}

#[test]
fn list_files_changed_only_lists_just_the_changed_files() {
    let d = Daemon::start();
    let mut c = d.client();
    let dir = repo(&d, "proj");
    put(&dir, "a.txt", "changed\n");
    std::fs::remove_file(dir.join("sub/b.txt")).unwrap();
    put(&dir, "deep/er/new.txt", "x\n");
    let s = open(&mut c, &dir);

    let t = tree(list(&mut c, &s, true));
    let files: Vec<_> = t.entries.iter().filter(|e| !e.dir).collect();
    let names: Vec<_> = files.iter().map(|e| e.path.as_str()).collect();
    assert_eq!(names, ["a.txt", "deep/er/new.txt", "sub/b.txt"]);
    assert_eq!(entry(&t, "sub/b.txt").status, Some(DiffStatus::Deleted));
    assert_eq!(entry(&t, "sub/b.txt").removed, 1);
    // the folders above a changed file are listed so a tree can be drawn
    assert!(entry(&t, "deep/er").dir);
    assert!(!paths(&t).contains(&".gitignore"));
}

#[test]
fn list_files_works_when_a_file_or_folder_is_named_like_a_revision() {
    let d = Daemon::start();
    let mut c = d.client();
    let dir = repo(&d, "proj");
    put(&dir, "HEAD", "not a revision\n"); // untracked: git sees HEAD as ref and path
    put(&dir, "main/x.txt", "x\n");
    let s = open(&mut c, &dir);

    let all = tree(list(&mut c, &s, false));
    assert_eq!(entry(&all, "HEAD").status, Some(DiffStatus::Untracked));
    let changed = tree(list(&mut c, &s, true));
    assert!(paths(&changed).contains(&"HEAD") && paths(&changed).contains(&"main/x.txt"));
    // and once tracked and changed
    git(&dir, &["add", "HEAD"]);
    git(&dir, &["commit", "-q", "-m", "head"]);
    put(&dir, "HEAD", "edited\n");
    let t = tree(list(&mut c, &s, true));
    assert_eq!(entry(&t, "HEAD").status, Some(DiffStatus::Modified));
    assert_eq!(entry(&t, "HEAD").added, 1);
}

#[test]
fn list_files_in_a_subfolder_session_is_relative_to_that_folder() {
    let d = Daemon::start();
    let mut c = d.client();
    let dir = repo(&d, "proj");
    put(&dir, "sub/b.txt", "bee\nbuzz\n");
    let s = open(&mut c, &dir.join("sub"));

    let t = tree(list(&mut c, &s, false));
    assert_eq!(paths(&t), ["b.txt"]);
    assert_eq!(entry(&t, "b.txt").status, Some(DiffStatus::Modified));
    assert_eq!(paths(&tree(list(&mut c, &s, true))), ["b.txt"]);
}

#[test]
fn list_files_of_a_plain_folder_walks_it_and_skips_dot_dirs() {
    let d = Daemon::start();
    let mut c = d.client();
    let dir = d.project_dir("plain");
    put(&dir, "x.txt", "x");
    put(&dir, "docs/y.md", "y");
    put(&dir, ".hidden/z", "z");
    let s = open(&mut c, &dir);

    let t = tree(list(&mut c, &s, false));
    assert!(!t.is_git && !t.truncated);
    assert_eq!(paths(&t), ["docs", "docs/y.md", "x.txt"]);
    assert!(tree(list(&mut c, &s, true)).entries.is_empty());
}

#[test]
fn list_files_of_an_empty_folder_is_an_empty_tree() {
    let d = Daemon::start();
    let mut c = d.client();
    let s = open(&mut c, &d.project_dir("empty"));
    let t = tree(list(&mut c, &s, false));
    assert!(t.entries.is_empty() && !t.truncated && !t.is_git);
}

#[test]
fn read_file_gives_text_sha_and_size_from_the_working_tree() {
    let d = Daemon::start();
    let mut c = d.client();
    let dir = repo(&d, "proj");
    put(&dir, "a.txt", "hello\nmore\n");
    let s = open(&mut c, &dir);

    let f = file(read(&mut c, &s, "a.txt", None));
    assert_eq!(
        (f.path.as_str(), f.text.as_str(), f.binary, f.size),
        ("a.txt", "hello\nmore\n", false, 11)
    );
    let g = file(read(&mut c, &s, "sub/b.txt", None));
    assert_eq!(g.text, "bee\n");
    // `./` and `.` parts are the same file
    assert_eq!(file(read(&mut c, &s, "./sub/./b.txt", None)).text, "bee\n");
}

#[test]
fn read_file_sha_is_the_sha256_of_the_bytes() {
    let d = Daemon::start();
    let mut c = d.client();
    let s = open(&mut c, &repo(&d, "proj"));
    assert_eq!(file(read(&mut c, &s, "a.txt", None)).sha, HELLO_SHA);
}

#[test]
fn read_file_at_head_gives_the_committed_text_not_the_working_tree() {
    let d = Daemon::start();
    let mut c = d.client();
    let dir = repo(&d, "proj");
    put(&dir, "a.txt", "edited\n");
    let s = open(&mut c, &dir);

    let at_head = file(read(&mut c, &s, "a.txt", Some("HEAD")));
    assert_eq!(at_head.text, "hello\n");
    assert_eq!(at_head.sha, HELLO_SHA);
    assert_eq!(file(read(&mut c, &s, "a.txt", None)).text, "edited\n");
    // a file deleted from the working tree is still there at HEAD
    std::fs::remove_file(dir.join("sub/b.txt")).unwrap();
    assert_eq!(
        file(read(&mut c, &s, "sub/b.txt", Some("HEAD"))).text,
        "bee\n"
    );
    // a subfolder session reads relative to its folder
    let sub = open(&mut c, &dir.join("sub"));
    assert_eq!(
        file(read(&mut c, &sub, "b.txt", Some("HEAD"))).text,
        "bee\n"
    );
}

#[test]
fn read_file_at_an_unknown_rev_or_missing_path_is_not_found() {
    let d = Daemon::start();
    let mut c = d.client();
    let s = open(&mut c, &repo(&d, "proj"));
    assert!(is_err(
        &read(&mut c, &s, "a.txt", Some("no-such-rev")),
        ErrorCode::NotFound
    ));
    assert!(is_err(
        &read(&mut c, &s, "nope.txt", Some("HEAD")),
        ErrorCode::NotFound
    ));
    // a folder is no file
    assert!(!matches!(
        read(&mut c, &s, "sub", Some("HEAD")),
        Response::File(_)
    ));
}

#[test]
fn a_rev_that_is_an_option_is_refused_and_git_never_sees_it() {
    let d = Daemon::start();
    let mut c = d.client();
    let dir = repo(&d, "proj");
    let s = open(&mut c, &dir);
    let pwned = d.tmp.path().join("pwned");
    for rev in [
        format!("--output={}", pwned.display()),
        "-p".to_owned(),
        "--batch".to_owned(),
        " --x".to_owned(),
        String::new(),
        "HEAD\0".to_owned(),
        "HEAD\n--x".to_owned(),
    ] {
        let r = read(&mut c, &s, "a.txt", Some(&rev));
        assert!(is_err(&r, ErrorCode::InvalidRequest), "{rev:?}: {r:?}");
    }
    assert!(!pwned.exists());
}

#[test]
fn rev_read_of_a_plain_folder_is_invalid() {
    let d = Daemon::start();
    let mut c = d.client();
    let dir = d.project_dir("plain");
    put(&dir, "x.txt", "x");
    let s = open(&mut c, &dir);
    assert!(is_err(
        &read(&mut c, &s, "x.txt", Some("HEAD")),
        ErrorCode::InvalidRequest
    ));
}

#[test]
fn write_file_replaces_the_text_and_returns_the_new_sha() {
    let d = Daemon::start();
    let mut c = d.client();
    let dir = repo(&d, "proj");
    let s = open(&mut c, &dir);

    let f = file(write(&mut c, &s, "a.txt", "new text\n", Some(HELLO_SHA)));
    assert_eq!(f.text, "new text\n");
    assert_eq!(
        std::fs::read_to_string(dir.join("a.txt")).unwrap(),
        "new text\n"
    );
    let again = file(read(&mut c, &s, "a.txt", None));
    assert_eq!(again.sha, f.sha);
    assert_ne!(f.sha, HELLO_SHA);
    // the new sha is the next write's guard; no sha means "overwrite"
    assert!(matches!(
        write(&mut c, &s, "a.txt", "third\n", Some(&f.sha)),
        Response::File(_)
    ));
    assert!(matches!(
        write(&mut c, &s, "a.txt", "fourth\n", None),
        Response::File(_)
    ));
    assert_eq!(
        std::fs::read_to_string(dir.join("a.txt")).unwrap(),
        "fourth\n"
    );
    // no temp file is left behind
    let left: Vec<_> = std::fs::read_dir(&dir)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .filter(|n| n.contains("lotse"))
        .collect();
    assert!(left.is_empty(), "{left:?}");
}

#[test]
fn write_file_with_a_stale_sha_is_a_conflict_and_changes_nothing() {
    let d = Daemon::start();
    let mut c = d.client();
    let dir = repo(&d, "proj");
    let s = open(&mut c, &dir);
    let seen = file(read(&mut c, &s, "a.txt", None)).sha;
    put(&dir, "a.txt", "someone else wrote this\n"); // the agent got there first

    let r = write(&mut c, &s, "a.txt", "mine\n", Some(&seen));
    assert!(is_err(&r, ErrorCode::Conflict), "{r:?}");
    assert_eq!(
        std::fs::read_to_string(dir.join("a.txt")).unwrap(),
        "someone else wrote this\n"
    );
}

#[test]
fn write_file_keeps_the_mode_and_writes_through_a_symlink_inside_the_folder() {
    use std::os::unix::fs::{PermissionsExt, symlink};
    let d = Daemon::start();
    let mut c = d.client();
    let dir = repo(&d, "proj");
    let script = dir.join("run.sh");
    put(&dir, "run.sh", "#!/bin/sh\n");
    std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
    symlink("a.txt", dir.join("link.txt")).unwrap();
    let s = open(&mut c, &dir);

    file(write(&mut c, &s, "run.sh", "#!/bin/sh\necho\n", None));
    let mode = std::fs::metadata(&script).unwrap().permissions().mode();
    assert_eq!(mode & 0o777, 0o755);

    file(write(&mut c, &s, "link.txt", "via link\n", None));
    assert!(
        std::fs::symlink_metadata(dir.join("link.txt"))
            .unwrap()
            .is_symlink(),
        "the link is still a link"
    );
    assert_eq!(
        std::fs::read_to_string(dir.join("a.txt")).unwrap(),
        "via link\n"
    );
}

#[test]
fn unknown_session_and_missing_file_are_not_found() {
    let d = Daemon::start();
    let mut c = d.client();
    let s = open(&mut c, &repo(&d, "proj"));
    let ghost = Session {
        id: lotse_proto::SessionId::new("nope"),
        ..s.clone()
    };
    assert!(is_err(&list(&mut c, &ghost, false), ErrorCode::NotFound));
    assert!(is_err(
        &read(&mut c, &ghost, "a.txt", None),
        ErrorCode::NotFound
    ));
    assert!(is_err(
        &write(&mut c, &ghost, "a.txt", "x", None),
        ErrorCode::NotFound
    ));
    assert!(is_err(
        &read(&mut c, &s, "missing.txt", None),
        ErrorCode::NotFound
    ));
    assert!(is_err(
        &write(&mut c, &s, "missing.txt", "x", None),
        ErrorCode::NotFound
    ));
}

/// Every way out of the folder or into `.git`, for read and write alike.
fn escapes(d: &Daemon, dir: &Path) -> Vec<String> {
    use std::os::unix::fs::symlink;
    let outside = d.tmp.path().join("outside");
    std::fs::create_dir_all(&outside).unwrap();
    std::fs::write(outside.join("secret.txt"), "top secret\n").unwrap();
    symlink(outside.join("secret.txt"), dir.join("leak.txt")).unwrap(); // a file link out
    symlink(&outside, dir.join("leakdir")).unwrap(); // a folder link out
    symlink("/etc/hosts", dir.join("hosts")).unwrap();
    symlink("../outside/secret.txt", dir.join("rel-leak.txt")).unwrap();
    symlink(".git", dir.join("gitlink")).unwrap(); // a link into .git
    [
        "../x",
        "../outside/secret.txt",
        "sub/../../outside/secret.txt",
        "sub/../../x",
        "..",
        "/etc/hosts",
        "/",
        "a\0.txt",
        "",
        "leak.txt",
        "leakdir/secret.txt",
        "hosts",
        "rel-leak.txt",
        ".git/config",
        ".git/hooks/pre-commit",
        ".GIT/config",
        "sub/.git/config",
        "gitlink/config",
    ]
    .map(str::to_owned)
    .to_vec()
}

#[test]
fn read_and_write_refuse_paths_that_leave_the_folder_or_enter_dot_git() {
    let d = Daemon::start();
    let mut c = d.client();
    let dir = repo(&d, "proj");
    let attempts = escapes(&d, &dir);
    let outside_before = std::fs::read_to_string(d.tmp.path().join("outside/secret.txt")).unwrap();
    let s = open(&mut c, &dir);
    for path in attempts {
        let r = read(&mut c, &s, &path, None);
        assert!(
            is_err(&r, ErrorCode::InvalidRequest),
            "read {path:?}: {r:?}"
        );
        let r = write(&mut c, &s, &path, "pwned\n", None);
        assert!(
            is_err(&r, ErrorCode::InvalidRequest),
            "write {path:?}: {r:?}"
        );
        // at a rev the paths git cannot know (links, `.git`) are not found, the rest is invalid
        let r = read(&mut c, &s, &path, Some("HEAD"));
        assert!(
            is_err(&r, ErrorCode::InvalidRequest) || is_err(&r, ErrorCode::NotFound),
            "rev {path:?}: {r:?}"
        );
    }
    assert_eq!(
        std::fs::read_to_string(d.tmp.path().join("outside/secret.txt")).unwrap(),
        outside_before
    );
    assert!(!d.tmp.path().join("x").exists());
    assert!(!dir.join(".git/hooks/pre-commit").exists());
    assert!(
        !std::fs::read_to_string(dir.join(".git/config"))
            .unwrap()
            .contains("pwned")
    );
}

#[test]
fn a_symlink_that_stays_inside_the_folder_is_read() {
    let d = Daemon::start();
    let mut c = d.client();
    let dir = repo(&d, "proj");
    std::os::unix::fs::symlink("sub", dir.join("alias")).unwrap();
    let s = open(&mut c, &dir);
    assert_eq!(file(read(&mut c, &s, "alias/b.txt", None)).text, "bee\n");
}

#[test]
fn a_folder_or_a_fifo_is_not_a_file() {
    let d = Daemon::start();
    let mut c = d.client();
    let dir = repo(&d, "proj");
    let fifo = dir.join("pipe");
    assert!(
        Command::new("mkfifo")
            .arg(&fifo)
            .status()
            .unwrap()
            .success()
    );
    let s = open(&mut c, &dir);
    for path in ["sub", "pipe"] {
        let r = read(&mut c, &s, path, None);
        assert!(is_err(&r, ErrorCode::InvalidRequest), "{path}: {r:?}");
        let r = write(&mut c, &s, path, "x", None);
        assert!(is_err(&r, ErrorCode::InvalidRequest), "{path}: {r:?}");
    }
}

#[test]
fn a_file_over_2_mib_has_no_text_and_cannot_be_written() {
    let d = Daemon::start();
    let mut c = d.client();
    let dir = repo(&d, "proj");
    put(&dir, "big.txt", &"a".repeat(MAX_BYTES + 1));
    put(&dir, "edge.txt", &"b".repeat(MAX_BYTES));
    let s = open(&mut c, &dir);

    let big = file(read(&mut c, &s, "big.txt", None));
    assert!(big.text.is_empty() && big.binary);
    assert_eq!(big.size, MAX_BYTES as u64 + 1);
    let r = write(&mut c, &s, "big.txt", "small\n", None);
    assert!(is_err(&r, ErrorCode::InvalidRequest), "{r:?}");
    assert_eq!(
        std::fs::metadata(dir.join("big.txt")).unwrap().len(),
        MAX_BYTES as u64 + 1
    );
    // exactly 2 MiB is still text
    let edge = file(read(&mut c, &s, "edge.txt", None));
    assert_eq!(edge.text.len(), MAX_BYTES);
    assert!(!edge.binary);
    // and so is the size of what is written
    let r = write(&mut c, &s, "a.txt", &"c".repeat(MAX_BYTES + 1), None);
    assert!(is_err(&r, ErrorCode::InvalidRequest), "{r:?}");
    assert_eq!(
        std::fs::read_to_string(dir.join("a.txt")).unwrap(),
        "hello\n"
    );
}

#[test]
fn a_big_committed_file_has_no_text_at_a_rev_either() {
    let d = Daemon::start();
    let mut c = d.client();
    let dir = repo(&d, "proj");
    put(&dir, "big.txt", &"a".repeat(MAX_BYTES + 1));
    git(&dir, &["add", "big.txt"]);
    git(&dir, &["commit", "-q", "-m", "big"]);
    let s = open(&mut c, &dir);
    let big = file(read(&mut c, &s, "big.txt", Some("HEAD")));
    assert!(big.text.is_empty() && big.binary);
}

#[test]
fn a_binary_file_reads_without_text_and_refuses_a_write() {
    let d = Daemon::start();
    let mut c = d.client();
    let dir = repo(&d, "proj");
    let bytes = [0x89u8, b'P', b'N', b'G', 0, 1, 2, 3];
    std::fs::write(dir.join("logo.png"), bytes).unwrap();
    std::fs::write(dir.join("latin1.txt"), [b'c', b'a', b'f', 0xE9]).unwrap();
    let s = open(&mut c, &dir);

    for name in ["logo.png", "latin1.txt"] {
        let f = file(read(&mut c, &s, name, None));
        assert!(f.binary && f.text.is_empty(), "{name}");
        let r = write(&mut c, &s, name, "text now\n", None);
        assert!(is_err(&r, ErrorCode::InvalidRequest), "{name}: {r:?}");
    }
    assert_eq!(std::fs::read(dir.join("logo.png")).unwrap(), bytes);
    assert_eq!(std::fs::read(dir.join("latin1.txt")).unwrap().len(), 4);
}
