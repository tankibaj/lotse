//! Alpha gaps (proto 3): workspaces, Show Changes (`get_diff`), Show Transcript (`get_transcript`)
//! and today's usage, through the real daemon on a temp data dir and a temp HOME.

mod common;

use common::*;
use lotse_proto::{
    AgentKind, DiffBase, DiffStatus, ErrorCode, Event, NewSession, Project, ProjectId, Request,
    Response, Session, Snapshot, Transcript, TranscriptRole, TranscriptSource, UsageToday,
    Workspace, WorkspaceId,
};
use std::path::Path;
use std::process::Command;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

fn error_code(r: Response) -> ErrorCode {
    match r {
        Response::Error(e) => e.code,
        other => panic!("expected an error, got {other:?}"),
    }
}

fn snapshot(c: &mut Client) -> Snapshot {
    match c.request(Request::GetSnapshot) {
        Response::Snapshot(s) => s,
        other => panic!("{other:?}"),
    }
}

fn create_workspace(c: &mut Client, name: &str) -> Workspace {
    match c.request(Request::CreateWorkspace { name: name.into() }) {
        Response::Workspace(w) => w,
        other => panic!("create_workspace {name}: {other:?}"),
    }
}

fn move_project(c: &mut Client, p: &Project, w: &Workspace) -> Response {
    c.request(Request::MoveProject {
        project_id: p.id.clone(),
        workspace_id: w.id.clone(),
    })
}

// ---- workspaces ---------------------------------------------------------------------------------

#[test]
fn a_fresh_daemon_has_the_default_workspace_and_projects_start_in_it() {
    let d = Daemon::start();
    let mut c = d.client();
    let s = snapshot(&mut c);
    assert_eq!(
        s.workspaces,
        [Workspace {
            id: WorkspaceId::new("w1"),
            name: "Default".into()
        }]
    );
    let p = c.add_project(&d.project_dir("a"));
    assert_eq!(p.workspace_id.as_str(), "w1");
}

#[test]
fn workspaces_are_created_renamed_moved_into_and_deleted_with_events() {
    let d = Daemon::start();
    let mut a = d.client();
    let mut b = d.client();
    let p = a.add_project(&d.project_dir("billing"));

    let w = create_workspace(&mut a, "  Client work ");
    assert_eq!(w.name, "Client work", "trimmed");
    assert!(matches!(
        b.wait_event(|e| matches!(e, Event::WorkspaceAdded(x) if *x == w)),
        Event::WorkspaceAdded(_)
    ));

    // names are unique (case-insensitive) and bounded
    for name in ["client WORK", "default"] {
        let r = a.request(Request::CreateWorkspace { name: name.into() });
        assert_eq!(error_code(r), ErrorCode::Conflict, "{name}");
    }
    for name in ["", "   ", &"x".repeat(101)] {
        let r = a.request(Request::CreateWorkspace { name: name.into() });
        assert_eq!(error_code(r), ErrorCode::InvalidRequest);
    }

    let Response::Workspace(renamed) = a.request(Request::RenameWorkspace {
        workspace_id: w.id.clone(),
        name: "Clients".into(),
    }) else {
        panic!("rename failed")
    };
    assert_eq!(renamed.name, "Clients");
    b.wait_event(|e| matches!(e, Event::WorkspaceUpdated(x) if x.name == "Clients"));
    let r = a.request(Request::RenameWorkspace {
        workspace_id: WorkspaceId::new("nope"),
        name: "X".into(),
    });
    assert_eq!(error_code(r), ErrorCode::NotFound);

    // move a project in, then out again
    let Response::Project(moved) = move_project(&mut a, &p, &w) else {
        panic!("move failed")
    };
    assert_eq!(moved.workspace_id, w.id);
    b.wait_event(|e| matches!(e, Event::ProjectUpdated(x) if x.workspace_id == w.id));
    assert_eq!(snapshot(&mut a).projects[0].workspace_id, w.id);
    let r = a.request(Request::MoveProject {
        project_id: ProjectId::new("nope"),
        workspace_id: w.id.clone(),
    });
    assert_eq!(error_code(r), ErrorCode::NotFound);
    let r = a.request(Request::MoveProject {
        project_id: p.id.clone(),
        workspace_id: WorkspaceId::new("nope"),
    });
    assert_eq!(error_code(r), ErrorCode::NotFound);

    // a workspace with projects is not deleted unless they move to the default one
    let r = a.request(Request::DeleteWorkspace {
        workspace_id: w.id.clone(),
        move_projects: false,
    });
    assert_eq!(error_code(r), ErrorCode::Conflict);
    assert_eq!(snapshot(&mut a).workspaces.len(), 2);
    a.expect_ok(Request::DeleteWorkspace {
        workspace_id: w.id.clone(),
        move_projects: true,
    });
    b.wait_event(|e| matches!(e, Event::ProjectUpdated(x) if x.id == p.id && x.workspace_id.as_str() == "w1"));
    b.wait_event(
        |e| matches!(e, Event::WorkspaceRemoved { workspace_id } if *workspace_id == w.id),
    );
    let s = snapshot(&mut a);
    assert_eq!(s.workspaces.len(), 1);
    assert_eq!(s.projects.len(), 1, "the project survived");
    assert_eq!(s.projects[0].workspace_id.as_str(), "w1");

    // an empty one goes without moving anything; the default one never goes
    let empty = create_workspace(&mut a, "Empty");
    a.expect_ok(Request::DeleteWorkspace {
        workspace_id: empty.id,
        move_projects: false,
    });
    let r = a.request(Request::DeleteWorkspace {
        workspace_id: WorkspaceId::new("w1"),
        move_projects: true,
    });
    assert_eq!(error_code(r), ErrorCode::Conflict);
}

#[test]
fn workspaces_and_project_placement_survive_a_restart() {
    let d = Daemon::start();
    let mut c = d.client();
    let p = c.add_project(&d.project_dir("billing"));
    let w = create_workspace(&mut c, "Client work");
    assert!(matches!(move_project(&mut c, &p, &w), Response::Project(_)));
    drop(c);
    let d = d.restart();
    let mut c = d.client();
    let s = snapshot(&mut c);
    assert_eq!(s.workspaces.len(), 2);
    assert_eq!(s.workspaces[1], w);
    assert_eq!(s.projects[0].workspace_id, w.id);
}

// ---- Show Changes ---------------------------------------------------------------------------------

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
fn get_diff_returns_the_session_folders_changes() {
    let d = Daemon::start();
    let mut c = d.client();
    let repo = d.project_dir("repo");
    git(&repo, &["init", "-q", "-b", "main"]);
    std::fs::write(repo.join("a.txt"), "one\n").unwrap();
    git(&repo, &["add", "."]);
    git(&repo, &["commit", "-q", "-m", "init"]);
    std::fs::write(repo.join("a.txt"), "one\ntwo\n").unwrap();
    std::fs::write(repo.join("new.txt"), "x\n").unwrap();
    let project = c.add_project(&repo);
    let session = c.create_session(&project, Some("/bin/sh"));

    let Response::Diff(diff) = c.request(Request::GetDiff {
        session_id: session.id.clone(),
        base: DiffBase::Head,
    }) else {
        panic!("no diff")
    };
    assert!(diff.is_git && !diff.truncated);
    let a = diff.files.iter().find(|f| f.path == "a.txt").unwrap();
    assert_eq!(a.status, DiffStatus::Modified);
    assert!(a.diff.contains("\n+two\n"), "{}", a.diff);
    let n = diff.files.iter().find(|f| f.path == "new.txt").unwrap();
    assert_eq!(n.status, DiffStatus::Untracked);
}

#[test]
fn get_diff_without_git_and_for_an_unknown_session() {
    let d = Daemon::start();
    let mut c = d.client();
    let plain = c.add_project(&d.project_dir("plain"));
    let session = c.create_session(&plain, Some("/bin/sh"));
    let Response::Diff(diff) = c.request(Request::GetDiff {
        session_id: session.id,
        base: DiffBase::Head,
    }) else {
        panic!("no diff")
    };
    assert!(!diff.is_git && diff.files.is_empty());
    let r = c.request(Request::GetDiff {
        session_id: "nope".into(),
        base: DiffBase::Head,
    });
    assert_eq!(error_code(r), ErrorCode::NotFound);
}

// ---- Show Transcript and usage ----------------------------------------------------------------------

fn agent_session(c: &mut Client, project: &Project, agent: AgentKind) -> Session {
    match c.request(Request::CreateSession(NewSession {
        project_id: project.id.clone(),
        agent,
        command: Some("/bin/sh".into()),
        title: None,
        worktree: None,
        first_prompt: None,
        parent_session: None,
        requested_by_agent: false,
    })) {
        Response::Session(s) => s,
        other => panic!("create_session: {other:?}"),
    }
}

fn hook(c: &mut Client, agent: AgentKind, session: &Session, transcript_path: &Path) {
    c.expect_ok(Request::HookEvent {
        agent,
        session_id: Some(session.id.clone()),
        payload: serde_json::json!({
            "hook_event_name": "UserPromptSubmit",
            "transcript_path": transcript_path,
        }),
    });
}

fn transcript(c: &mut Client, session: &Session) -> Response {
    c.request(Request::GetTranscript {
        session_id: session.id.clone(),
        limit: None,
    })
}

/// `now` as the agents write timestamps (UTC, ISO 8601).
fn iso_now() -> String {
    let ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_millis() as i64;
    let (secs, ms) = (ms / 1000, ms % 1000);
    let (days, rem) = (secs.div_euclid(86_400), secs.rem_euclid(86_400));
    // civil from days (Howard Hinnant)
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    format!(
        "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}.{ms:03}Z",
        rem / 3600,
        rem % 3600 / 60,
        rem % 60
    )
}

fn claude_transcript(now: &str) -> String {
    format!(
        "{{\"type\":\"user\",\"timestamp\":\"{now}\",\"message\":{{\"role\":\"user\",\"content\":\"Fix the rounding\"}}}}\n\
         {{\"type\":\"assistant\",\"timestamp\":\"{now}\",\"requestId\":\"r1\",\"message\":{{\"id\":\"m1\",\"model\":\"claude-sonnet-4-5\",\"role\":\"assistant\",\"content\":[{{\"type\":\"text\",\"text\":\"Looking.\"}},{{\"type\":\"tool_use\",\"name\":\"Bash\",\"input\":{{\"command\":\"cargo test\"}}}}],\"usage\":{{\"input_tokens\":1000000,\"output_tokens\":100000,\"cache_read_input_tokens\":0,\"cache_creation_input_tokens\":0}}}}}}\n"
    )
}

#[test]
fn a_claude_transcript_announced_by_a_hook_can_be_shown() {
    let d = Daemon::start();
    let mut c = d.client();
    let project = c.add_project(&d.project_dir("proj"));
    let session = agent_session(&mut c, &project, AgentKind::ClaudeCode);

    // before any hook: nothing to show yet, but not an error
    let Response::Transcript(t) = transcript(&mut c, &session) else {
        panic!("no transcript answer")
    };
    assert_eq!(
        t,
        Transcript {
            available: false,
            turns: vec![],
            truncated: false,
            source: TranscriptSource::Claude,
        }
    );

    let file = d.home.join(".claude/projects/p/s.jsonl");
    std::fs::create_dir_all(file.parent().unwrap()).unwrap();
    std::fs::write(&file, claude_transcript(&iso_now())).unwrap();
    hook(&mut c, AgentKind::ClaudeCode, &session, &file);

    let Response::Transcript(t) = transcript(&mut c, &session) else {
        panic!("no transcript answer")
    };
    assert!(t.available);
    let roles: Vec<_> = t.turns.iter().map(|t| t.role).collect();
    assert_eq!(
        roles,
        [
            TranscriptRole::User,
            TranscriptRole::Assistant,
            TranscriptRole::Tool
        ]
    );
    assert_eq!(t.turns[2].tool.as_ref().unwrap().summary, "cargo test");

    // the limit keeps the newest turns
    let Response::Transcript(t) = c.request(Request::GetTranscript {
        session_id: session.id.clone(),
        limit: Some(1),
    }) else {
        panic!()
    };
    assert_eq!(t.turns.len(), 1);
    assert!(t.truncated);
}

#[test]
fn a_hook_cannot_point_the_daemon_at_files_outside_home() {
    let d = Daemon::start();
    let mut c = d.client();
    let project = c.add_project(&d.project_dir("proj"));
    let session = agent_session(&mut c, &project, AgentKind::ClaudeCode);
    let outside = tempfile::tempdir().unwrap();
    let secret = outside.path().join("secret.jsonl");
    std::fs::write(&secret, claude_transcript(&iso_now())).unwrap();
    let not_jsonl = d.home.join("notes.txt");
    std::fs::write(&not_jsonl, claude_transcript(&iso_now())).unwrap();
    let link = d.home.join("link.jsonl");
    std::os::unix::fs::symlink(&secret, &link).unwrap();

    for path in [
        secret.as_path(),
        not_jsonl.as_path(),
        link.as_path(),
        Path::new("relative.jsonl"),
    ] {
        hook(&mut c, AgentKind::ClaudeCode, &session, path);
        let Response::Transcript(t) = transcript(&mut c, &session) else {
            panic!("no transcript answer")
        };
        assert!(!t.available && t.turns.is_empty(), "{path:?} was accepted");
    }
}

#[test]
fn agents_without_a_transcript_show_their_scrollback() {
    let d = Daemon::start();
    let mut c = d.client();
    let project = c.add_project(&d.project_dir("proj"));
    for agent in [AgentKind::OpenCode, AgentKind::Terminal] {
        let session = agent_session(&mut c, &project, agent);
        // OpenCode's plugin payload has no transcript path
        c.expect_ok(Request::HookEvent {
            agent,
            session_id: Some(session.id.clone()),
            payload: serde_json::json!({"hook_event_name": "SessionIdle", "session_id": "x"}),
        });
        // since Beta (wp07) the terminal's own scrollback stands in for the transcript
        match transcript(&mut c, &session) {
            Response::Transcript(t) => {
                assert!(t.available, "{agent:?}");
                assert_eq!(t.source, TranscriptSource::Scrollback, "{agent:?}");
            }
            other => panic!("{other:?}"),
        }
    }
    let r = c.request(Request::GetTranscript {
        session_id: "nope".into(),
        limit: None,
    });
    assert_eq!(error_code(r), ErrorCode::NotFound);
}

#[test]
fn a_codex_rollout_announced_by_a_hook_can_be_shown() {
    let d = Daemon::start();
    let mut c = d.client();
    let project = c.add_project(&d.project_dir("proj"));
    let session = agent_session(&mut c, &project, AgentKind::Codex);
    let file = d.home.join(".codex/sessions/2026/11/02/rollout-x.jsonl");
    std::fs::create_dir_all(file.parent().unwrap()).unwrap();
    std::fs::write(
        &file,
        "{\"type\":\"response_item\",\"payload\":{\"type\":\"message\",\"role\":\"user\",\"content\":[{\"type\":\"input_text\",\"text\":\"add a test\"}]}}\n",
    )
    .unwrap();
    hook(&mut c, AgentKind::Codex, &session, &file);
    let Response::Transcript(t) = transcript(&mut c, &session) else {
        panic!("no transcript answer")
    };
    assert_eq!(t.turns.len(), 1);
    assert_eq!(t.turns[0].text, "add a test");
}

fn wait_usage(c: &mut Client, want_tokens: u64) -> UsageToday {
    let end = Instant::now() + Duration::from_secs(15);
    loop {
        let s = snapshot(c);
        if s.usage.total_tokens() == want_tokens {
            return s.usage;
        }
        assert!(Instant::now() < end, "usage stayed at {:?}", s.usage);
        std::thread::sleep(Duration::from_millis(250));
    }
}

#[test]
fn todays_usage_follows_the_announced_transcripts_pushes_events_and_survives_a_restart() {
    let d = Daemon::start();
    let mut c = d.client();
    let project = c.add_project(&d.project_dir("proj"));
    let session = agent_session(&mut c, &project, AgentKind::ClaudeCode);
    let none = snapshot(&mut c).usage;
    assert_eq!((none.total_tokens(), none.cost_micros), (0, Some(0)));

    let file = d.home.join(".claude/projects/p/s.jsonl");
    std::fs::create_dir_all(file.parent().unwrap()).unwrap();
    std::fs::write(&file, claude_transcript(&iso_now())).unwrap();
    hook(&mut c, AgentKind::ClaudeCode, &session, &file);

    let ev = c.wait_event(|e| matches!(e, Event::UsageChanged(_)));
    let Event::UsageChanged(u) = ev else {
        unreachable!()
    };
    assert_eq!((u.input_tokens, u.output_tokens), (1_000_000, 100_000));
    // sonnet 4.5: $3 in, $15 out per million
    assert_eq!(u.cost_micros, Some(3_000_000 + 1_500_000));
    assert_eq!(snapshot(&mut c).usage, u);

    // more output is picked up and pushed again
    let more = format!(
        "{}{{\"type\":\"assistant\",\"timestamp\":\"{}\",\"requestId\":\"r2\",\"message\":{{\"id\":\"m2\",\"model\":\"claude-sonnet-4-5\",\"content\":[],\"usage\":{{\"input_tokens\":1000,\"output_tokens\":0}}}}}}\n",
        std::fs::read_to_string(&file).unwrap(),
        iso_now()
    );
    std::fs::write(&file, more).unwrap();
    let ev = c.wait_event(|e| matches!(e, Event::UsageChanged(u) if u.input_tokens == 1_001_000));
    assert!(matches!(ev, Event::UsageChanged(_)));

    // a restarted daemon follows the transcripts of the run before
    drop(c);
    let d = d.restart();
    let mut c = d.client();
    let u = wait_usage(&mut c, 1_101_000);
    assert_eq!(u.cost_micros, Some(3_000_000 + 3_000 + 1_500_000));
}
