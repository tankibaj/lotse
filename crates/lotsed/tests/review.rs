//! Beta wp04: `get_review`, `get_diff` against the merge-base, and the behind / +− line of
//! `get_git_info`, through the socket.
mod common;

use common::{Client, Daemon};
use lotse_proto::{DiffBase, DiffStatus, ErrorCode, Request, Response, Session};
use std::path::Path;
use std::process::Command;

fn git(dir: &Path, args: &[&str]) {
    let ok = Command::new("git")
        .args(["-c", "commit.gpgsign=false"])
        .args(args)
        .current_dir(dir)
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_SYSTEM", "/dev/null")
        .env("GIT_AUTHOR_NAME", "Ada")
        .env("GIT_AUTHOR_EMAIL", "t@t")
        .env("GIT_COMMITTER_NAME", "Ada")
        .env("GIT_COMMITTER_EMAIL", "t@t")
        .status()
        .unwrap()
        .success();
    assert!(ok, "git {args:?}");
}

fn commit(dir: &Path, name: &str, text: &str, msg: &str) {
    std::fs::write(dir.join(name), text).unwrap();
    git(dir, &["add", "."]);
    git(dir, &["commit", "-q", "-m", msg]);
}

/// `feature` is 2 commits ahead of and 1 behind `main`; checked out.
fn diverged_repo(d: &Daemon, name: &str) -> std::path::PathBuf {
    let repo = d.project_dir(name);
    git(&repo, &["init", "-q", "-b", "main"]);
    commit(&repo, "a.txt", "a\n", "init");
    git(&repo, &["checkout", "-q", "-b", "feature"]);
    commit(&repo, "f1.txt", "one\n", "add f1");
    commit(&repo, "f2.txt", "two\ntwo\n", "add f2");
    git(&repo, &["checkout", "-q", "main"]);
    commit(&repo, "m.txt", "m\n", "main moves");
    git(&repo, &["checkout", "-q", "feature"]);
    repo
}

fn session_in(c: &mut Client, dir: &Path) -> Session {
    let project = c.add_project(dir);
    c.create_session(&project, Some("/bin/sh"))
}

fn error_code(r: Response) -> ErrorCode {
    match r {
        Response::Error(e) => e.code,
        other => panic!("expected an error, got {other:?}"),
    }
}

fn get_review(c: &mut Client, s: &Session) -> lotse_proto::Review {
    match c.request(Request::GetReview {
        session_id: s.id.clone(),
    }) {
        Response::Review(r) => r,
        other => panic!("get_review: {other:?}"),
    }
}

#[test]
fn review_lists_commits_ahead_of_base() {
    let d = Daemon::start();
    let mut c = d.client();
    let repo = diverged_repo(&d, "repo");
    let s = session_in(&mut c, &repo);
    let r = get_review(&mut c, &s);
    assert_eq!(r.base, "main");
    assert_eq!((r.ahead, r.behind), (2, 1));
    assert!(!r.truncated);
    let subjects: Vec<_> = r.commits.iter().map(|c| c.subject.as_str()).collect();
    assert_eq!(subjects, ["add f2", "add f1"]);
    assert_eq!(r.commits[0].author, "Ada");
}

#[test]
fn review_of_an_unknown_session_is_not_found() {
    let d = Daemon::start();
    let mut c = d.client();
    let r = c.request(Request::GetReview {
        session_id: "nope".into(),
    });
    assert_eq!(error_code(r), ErrorCode::NotFound);
    let r = c.request(Request::GetDiff {
        session_id: "nope".into(),
        base: DiffBase::MergeBase,
    });
    assert_eq!(error_code(r), ErrorCode::NotFound);
}

#[test]
fn review_of_a_folder_without_git_has_no_base_and_is_not_an_error() {
    let d = Daemon::start();
    let mut c = d.client();
    let s = session_in(&mut c, &d.project_dir("plain"));
    let r = get_review(&mut c, &s);
    assert_eq!(
        (r.base.as_str(), r.ahead, r.behind, r.commits.len()),
        ("", 0, 0, 0)
    );
    let Response::Diff(diff) = c.request(Request::GetDiff {
        session_id: s.id,
        base: DiffBase::MergeBase,
    }) else {
        panic!("no diff")
    };
    assert!(!diff.is_git && diff.files.is_empty());
}

#[test]
fn review_without_a_default_branch_has_an_empty_base() {
    let d = Daemon::start();
    let mut c = d.client();
    let repo = d.project_dir("trunk");
    git(&repo, &["init", "-q", "-b", "trunk"]);
    commit(&repo, "a.txt", "a\n", "init");
    let s = session_in(&mut c, &repo);
    let r = get_review(&mut c, &s);
    assert_eq!(
        (r.base.as_str(), r.ahead, r.behind, r.commits.len()),
        ("", 0, 0, 0)
    );
}

#[test]
fn diff_from_merge_base_counts_lines() {
    let d = Daemon::start();
    let mut c = d.client();
    let repo = diverged_repo(&d, "repo");
    std::fs::write(repo.join("f2.txt"), "two\ntwo\nthree\n").unwrap(); // uncommitted +1
    let s = session_in(&mut c, &repo);
    let Response::Diff(diff) = c.request(Request::GetDiff {
        session_id: s.id.clone(),
        base: DiffBase::MergeBase,
    }) else {
        panic!("no diff")
    };
    assert!(diff.is_git && !diff.truncated);
    let file = |p: &str| diff.files.iter().find(|f| f.path == p).unwrap();
    assert_eq!(file("f1.txt").status, DiffStatus::Added);
    assert_eq!((file("f1.txt").added, file("f1.txt").removed), (1, 0));
    assert_eq!((file("f2.txt").added, file("f2.txt").removed), (3, 0));
    assert!(diff.files.iter().all(|f| f.path != "m.txt"), "{diff:?}");

    // against HEAD only the uncommitted change shows
    let Response::Diff(head) = c.request(Request::GetDiff {
        session_id: s.id,
        base: DiffBase::Head,
    }) else {
        panic!("no diff")
    };
    assert_eq!(head.files.len(), 1);
    assert_eq!(head.files[0].path, "f2.txt");
    assert_eq!((head.files[0].added, head.files[0].removed), (1, 0));
}

#[test]
fn git_info_carries_behind_and_the_uncommitted_counts() {
    let d = Daemon::start();
    let mut c = d.client();
    let repo = diverged_repo(&d, "repo");
    std::fs::write(repo.join("f1.txt"), "changed\n").unwrap(); // -1 +1
    std::fs::write(repo.join("f2.txt"), "two\n").unwrap(); // -1
    let project = c.add_project(&repo);
    let s = c.create_session(&project, Some("/bin/sh"));
    let Response::GitInfo(g) = c.request(Request::GetGitInfo {
        project_id: project.id,
        session_id: Some(s.id),
    }) else {
        panic!("no git info")
    };
    assert_eq!((g.behind, g.added, g.removed), (1, 1, 2));
    assert_eq!(g.pr, None);
}
