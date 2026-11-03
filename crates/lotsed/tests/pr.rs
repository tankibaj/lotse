//! Beta wp04: PR status through fake `gh` / `glab` binaries on the daemon's PATH. No real network,
//! no real CLI: the fakes record their argv and print canned JSON.
mod common;

use common::{Client, Daemon, script};
use lotse_proto::{
    Event, GitInfo, PrChecks, PrProvider, PrState, Project, Request, Response, Session, Settings,
};
use std::path::{Path, PathBuf};
use std::process::Command;
use tempfile::TempDir;

const GH_OPEN: &str = r#"{"number":42,"url":"https://github.com/o/r/pull/42","state":"OPEN","isDraft":false,"statusCheckRollup":[{"__typename":"CheckRun","status":"COMPLETED","conclusion":"SUCCESS"}]}"#;
const GLAB_DRAFT: &str = r#"{"iid":7,"web_url":"https://gitlab.example.com/o/r/-/merge_requests/7","state":"opened","draft":true,"head_pipeline":{"status":"failed"}}"#;

fn git(dir: &Path, args: &[&str]) {
    let ok = Command::new("git")
        .args(["-c", "commit.gpgsign=false"])
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

/// Fake CLIs: each call appends its argv to `<bin>/<name>.log` and answers with `body`.
struct Fakes {
    dir: TempDir,
}

impl Fakes {
    fn new() -> Self {
        Self {
            dir: tempfile::tempdir().unwrap(),
        }
    }

    fn fake(&self, name: &str, body: &str) {
        let log = self.log_path(name);
        script(
            self.dir.path(),
            name,
            &format!("printf '%s\\n' \"$*\" >> '{}'\n{body}", log.display()),
        );
    }

    fn log_path(&self, name: &str) -> PathBuf {
        self.dir.path().join(format!("{name}.log"))
    }

    fn calls(&self, name: &str) -> Vec<String> {
        std::fs::read_to_string(self.log_path(name))
            .map(|s| s.lines().map(str::to_owned).collect())
            .unwrap_or_default()
    }
}

/// A repo on `feature` with `origin` pointing at `url` (never contacted).
fn repo(d: &Daemon, url: &str, branch: &str) -> PathBuf {
    let repo = d.project_dir("repo");
    git(&repo, &["init", "-q", "-b", "main"]);
    git(&repo, &["commit", "-q", "--allow-empty", "-m", "init"]);
    if branch != "main" {
        git(&repo, &["checkout", "-q", "-b", branch]);
    }
    git(&repo, &["config", "remote.origin.url", url]);
    repo
}

fn enable_pr_status(c: &mut Client) {
    let Response::Settings(mut s) = c.request(Request::GetSettings) else {
        panic!("no settings")
    };
    s.external.pr_status = true;
    assert!(matches!(
        c.request(Request::UpdateSettings { settings: s }),
        Response::Settings(Settings { .. })
    ));
}

fn open(c: &mut Client, repo: &Path) -> (Project, Session) {
    let project = c.add_project(repo);
    let session = c.create_session(&project, Some("/bin/sh"));
    (project, session)
}

fn git_info(c: &mut Client, p: &Project, s: &Session) -> GitInfo {
    match c.request(Request::GetGitInfo {
        project_id: p.id.clone(),
        session_id: Some(s.id.clone()),
    }) {
        Response::GitInfo(g) => g,
        other => panic!("get_git_info: {other:?}"),
    }
}

#[test]
fn pr_status_off_by_default_runs_nothing() {
    let fakes = Fakes::new();
    fakes.fake("gh", &format!("echo '{GH_OPEN}'"));
    fakes.fake("glab", &format!("echo '{GLAB_DRAFT}'"));
    let d = Daemon::start_with_path(Some(fakes.dir.path()));
    let mut c = d.client();
    let (p, s) = open(&mut c, &repo(&d, "https://github.com/o/r.git", "feature"));
    assert_eq!(git_info(&mut c, &p, &s).pr, None);
    // the periodic refresh does not run them either
    c.drain(std::time::Duration::from_millis(6500));
    assert!(fakes.calls("gh").is_empty() && fakes.calls("glab").is_empty());
}

#[test]
fn pr_status_from_gh() {
    let fakes = Fakes::new();
    fakes.fake("gh", &format!("echo '{GH_OPEN}'"));
    let d = Daemon::start_with_path(Some(fakes.dir.path()));
    let mut c = d.client();
    enable_pr_status(&mut c);
    let (p, s) = open(&mut c, &repo(&d, "https://github.com/o/r.git", "feature"));
    let pr = git_info(&mut c, &p, &s).pr.expect("a pull request");
    assert_eq!(
        (pr.number, pr.state, pr.checks, pr.provider),
        (42, PrState::Open, Some(PrChecks::Pass), PrProvider::Github)
    );
    assert_eq!(pr.url, "https://github.com/o/r/pull/42");
    assert_eq!(
        fakes.calls("gh"),
        ["pr view --json number,url,state,isDraft,statusCheckRollup -- feature"]
    );
    // cached for 60 s: asking again runs nothing
    assert!(git_info(&mut c, &p, &s).pr.is_some());
    assert_eq!(fakes.calls("gh").len(), 1);
    // and the periodic refresh pushes the change to clients
    let e = c.wait_event(|e| matches!(e, Event::GitInfoChanged { git, .. } if git.pr.is_some()));
    assert!(matches!(e, Event::GitInfoChanged { session_id, .. } if session_id == s.id));
    assert_eq!(fakes.calls("gh").len(), 1);
}

#[test]
fn pr_status_from_glab() {
    let fakes = Fakes::new();
    // gh knows no gitlab host
    fakes.fake("gh", "exit 1");
    fakes.fake("glab", &format!("echo '{GLAB_DRAFT}'"));
    let d = Daemon::start_with_path(Some(fakes.dir.path()));
    let mut c = d.client();
    enable_pr_status(&mut c);
    let url = "git@gitlab.example.com:o/r.git";
    let (p, s) = open(&mut c, &repo(&d, url, "feature"));
    let pr = git_info(&mut c, &p, &s).pr.expect("a merge request");
    assert_eq!(
        (pr.number, pr.state, pr.checks, pr.provider),
        (7, PrState::Draft, Some(PrChecks::Fail), PrProvider::Gitlab)
    );
    assert_eq!(
        fakes.calls("gh"),
        ["auth status --hostname gitlab.example.com"]
    );
    assert_eq!(fakes.calls("glab"), ["mr view -F json -- feature"]);
}

#[test]
fn pr_status_uses_gh_for_a_host_it_is_logged_in_to() {
    let fakes = Fakes::new();
    fakes.fake(
        "gh",
        &format!("[ \"$1\" = auth ] && exit 0\necho '{GH_OPEN}'"),
    );
    fakes.fake("glab", "exit 1");
    let d = Daemon::start_with_path(Some(fakes.dir.path()));
    let mut c = d.client();
    enable_pr_status(&mut c);
    let (p, s) = open(
        &mut c,
        &repo(&d, "https://git.corp.example/o/r.git", "feature"),
    );
    let pr = git_info(&mut c, &p, &s).pr.expect("a pull request");
    assert_eq!(pr.provider, PrProvider::Github);
    assert!(fakes.calls("glab").is_empty());
}

#[test]
fn pr_status_timeout_leaves_none() {
    let fakes = Fakes::new();
    fakes.fake("gh", "exec sleep 30");
    let d = Daemon::start_with_path(Some(fakes.dir.path()));
    let mut c = d.client();
    enable_pr_status(&mut c);
    let (p, s) = open(&mut c, &repo(&d, "https://github.com/o/r.git", "feature"));
    let started = std::time::Instant::now();
    assert_eq!(git_info(&mut c, &p, &s).pr, None);
    assert!(started.elapsed() < std::time::Duration::from_secs(8));
    // the miss is cached too: no retry storm
    assert_eq!(git_info(&mut c, &p, &s).pr, None);
    assert_eq!(fakes.calls("gh").len(), 1);
}

#[test]
fn pr_status_without_a_pull_request_or_remote_is_none() {
    let fakes = Fakes::new();
    fakes.fake("gh", "echo 'no pull requests found' >&2\nexit 1");
    let d = Daemon::start_with_path(Some(fakes.dir.path()));
    let mut c = d.client();
    enable_pr_status(&mut c);
    let (p, s) = open(&mut c, &repo(&d, "https://github.com/o/r.git", "feature"));
    assert_eq!(git_info(&mut c, &p, &s).pr, None);
    assert_eq!(fakes.calls("gh").len(), 1);

    // no origin: nothing to ask
    let dir = d.project_dir("local");
    git(&dir, &["init", "-q", "-b", "main"]);
    git(&dir, &["commit", "-q", "--allow-empty", "-m", "init"]);
    let (p, s) = open(&mut c, &dir);
    assert_eq!(git_info(&mut c, &p, &s).pr, None);
    assert_eq!(fakes.calls("gh").len(), 1);
}

#[test]
fn a_branch_named_help_reaches_gh_as_a_value() {
    let fakes = Fakes::new();
    fakes.fake("gh", &format!("echo '{GH_OPEN}'"));
    let d = Daemon::start_with_path(Some(fakes.dir.path()));
    let mut c = d.client();
    enable_pr_status(&mut c);
    let repo = repo(&d, "https://github.com/o/r.git", "main");
    git(&repo, &["symbolic-ref", "HEAD", "refs/heads/--help"]);
    let (p, s) = open(&mut c, &repo);
    assert!(git_info(&mut c, &p, &s).pr.is_some());
    assert_eq!(
        fakes.calls("gh"),
        ["pr view --json number,url,state,isDraft,statusCheckRollup -- --help"]
    );
}
