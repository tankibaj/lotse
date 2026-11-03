//! Beta wave 0 on the real daemon: every new request is answered (`unavailable` until its WP lands),
//! a stale `prompt_id` is a `conflict` that types nothing, and the hub observer sees events.
//! Temp data dir, temp HOME: nothing of the real `~/.claude` or Lotse data is touched.

mod common;

use common::{Client, Daemon};
use lotse_proto::{
    AgentKind, DiffBase, ErrorCode, Event, ExportFormat, NewSession, PermDecision, PermVia,
    Request, Response, Session, SessionId, Status,
};
use serde_json::json;
use std::time::Duration;

fn unavailable(r: &Response) -> bool {
    matches!(r, Response::Error(e) if e.code == ErrorCode::Unavailable)
}

fn conflict(r: &Response) -> bool {
    matches!(r, Response::Error(e) if e.code == ErrorCode::Conflict)
}

#[test]
fn every_new_request_answers_unavailable_until_its_wp_lands() {
    let d = Daemon::start();
    let mut c = d.client();
    let session_id = SessionId::new("nope");
    let s = || session_id.clone();
    let requests = [
        Request::GetReview { session_id: s() },
        Request::ListFiles {
            session_id: s(),
            changed_only: false,
        },
        Request::ReadFile {
            session_id: s(),
            path: "a.txt".into(),
            rev: None,
        },
        Request::WriteFile {
            session_id: s(),
            path: "a.txt".into(),
            text: "x".into(),
            expected_sha: None,
        },
        Request::GetUsage { session_id: s() },
        Request::ExportTranscript {
            session_id: s(),
            format: ExportFormat::Markdown,
        },
        Request::GetSessionInfo { session_id: s() },
        Request::StopTask {
            session_id: s(),
            pid: 1,
        },
        Request::ListConnectors { session_id: s() },
        Request::CheckConnectors { session_id: s() },
    ];
    for req in requests {
        let method = req.method();
        let resp = c.request(req);
        assert!(unavailable(&resp), "{method}: {resp:?}");
    }
    // get_diff against HEAD keeps working; against the merge-base it waits for wp04
    let diff = |c: &mut Client, base| {
        c.request(Request::GetDiff {
            session_id: s(),
            base,
        })
    };
    let resp = diff(&mut c, DiffBase::Head);
    assert!(
        matches!(&resp, Response::Error(e) if e.code == ErrorCode::NotFound),
        "{resp:?}"
    );
    assert!(unavailable(&diff(&mut c, DiffBase::MergeBase)));
}

fn agent_session(c: &mut Client, d: &Daemon, command: &str) -> Session {
    let project = c.add_project(&d.project_dir("praxis-web"));
    match c.request(Request::CreateSession(NewSession {
        project_id: project.id.clone(),
        agent: AgentKind::ClaudeCode,
        command: Some(command.into()),
        title: None,
        worktree: None,
        first_prompt: None,
    })) {
        Response::Session(s) => s,
        other => panic!("create_session: {other:?}"),
    }
}

fn hook(c: &mut Client, s: &Session, payload: serde_json::Value) {
    c.expect_ok(Request::HookEvent {
        agent: AgentKind::ClaudeCode,
        session_id: Some(s.id.clone()),
        payload,
    });
}

fn status_is(want: Status) -> impl Fn(&Event) -> bool {
    move |e| matches!(e, Event::StatusChanged { status, .. } if *status == want)
}

/// A pane that shows a prompt, then reports the hex of the first byte it reads: the assertion is
/// on the bytes that reached the PTY. The directory holds the script: keep it alive.
fn prompt_pane(d: &Daemon, c: &mut Client) -> (Session, tempfile::TempDir) {
    let dir = tempfile::tempdir().unwrap();
    common::script(
        dir.path(),
        "agent.sh",
        "stty -icanon -echo min 1\n\
         printf ' Do you want to proceed?\\n ❯ 1. Yes\\n   2. No\\n'\n\
         printf 'BYTES:%s\\n' \"$(dd bs=1 count=1 2>/dev/null | od -An -tx1 | tr -d ' \\n')\"\nsleep 5",
    );
    let s = agent_session(c, d, &dir.path().join("agent.sh").to_string_lossy());
    c.attach(&s.id, 80, 24);
    assert!(c.wait_output(&s.id, "2. No"));
    (s, dir)
}

/// The id the daemon put on the open permission request.
fn ask_permission(c: &mut Client, s: &Session, command: &str) -> String {
    hook(
        c,
        s,
        json!({"hook_event_name": "PermissionRequest", "tool_name": "Bash",
            "tool_input": {"command": command}}),
    );
    let Event::StatusChanged { perm, .. } = c.wait_event(status_is(Status::Perm)) else {
        unreachable!()
    };
    perm.expect("perm on the event").id.expect("a prompt id")
}

fn resolve(c: &mut Client, s: &Session, prompt_id: Option<&str>) -> Response {
    c.request(Request::ResolvePermission {
        session_id: s.id.clone(),
        decision: PermDecision::Allow,
        via: Some(PermVia::Notification),
        prompt_id: prompt_id.map(str::to_owned),
    })
}

#[test]
fn stale_prompt_id_is_a_conflict_and_types_nothing() {
    let d = Daemon::start();
    let mut c = d.client();
    let (s, _dir) = prompt_pane(&d, &mut c);
    let id = ask_permission(&mut c, &s, "ls");
    assert!(conflict(&resolve(
        &mut c,
        &s,
        Some("a-notification-of-an-older-prompt")
    )));
    assert_ne!(id, "a-notification-of-an-older-prompt");
    // the first byte the pane reads is the one typed now: nothing was typed before
    c.expect_ok(Request::SendInput {
        session_id: s.id.clone(),
        text: "z".into(),
    });
    assert!(c.wait_output(&s.id, "BYTES:7a"));
}

#[test]
fn the_current_prompt_id_answers_the_prompt() {
    let d = Daemon::start();
    let mut c = d.client();
    let (s, _dir) = prompt_pane(&d, &mut c);
    let id = ask_permission(&mut c, &s, "ls");
    assert!(matches!(resolve(&mut c, &s, Some(&id)), Response::Ok));
    assert!(c.wait_output(&s.id, "BYTES:31"));
}

#[test]
fn a_request_without_prompt_id_still_answers_the_prompt() {
    let d = Daemon::start();
    let mut c = d.client();
    let (s, _dir) = prompt_pane(&d, &mut c);
    ask_permission(&mut c, &s, "ls");
    assert!(matches!(resolve(&mut c, &s, None), Response::Ok));
    assert!(c.wait_output(&s.id, "BYTES:31"));
}

#[test]
fn each_new_prompt_gets_its_own_id() {
    let d = Daemon::start();
    let mut c = d.client();
    let s = agent_session(&mut c, &d, "/bin/sh");
    let first = ask_permission(&mut c, &s, "ls");
    hook(&mut c, &s, json!({"hook_event_name": "UserPromptSubmit"}));
    c.wait_event(status_is(Status::Run));
    let second = ask_permission(&mut c, &s, "ls");
    assert_ne!(
        first, second,
        "the same command asked again is a new prompt"
    );
    // the id also rides on the session in the list
    let Response::Sessions(list) = c.request(Request::ListSessions { project_id: None }) else {
        panic!("list_sessions")
    };
    assert_eq!(
        list[0].perm.as_ref().and_then(|p| p.id.as_deref()),
        Some(second.as_str())
    );
}

#[test]
fn a_hub_observer_sees_status_changed() {
    use lotsed::config::Config;
    use lotsed::db::Db;
    use lotsed::state::State;

    let tmp = tempfile::tempdir().unwrap();
    let home = tmp.path().join("home");
    std::fs::create_dir_all(&home).unwrap();
    let mut cfg = Config::new(tmp.path().join("data"), false);
    cfg.home = Some(home);
    let state = State::new(cfg, Db::open_in_memory().unwrap()).unwrap();

    let (tx, rx) = std::sync::mpsc::channel();
    let tx = std::sync::Mutex::new(tx);
    state.hub.observe(Box::new(move |e: &Event| {
        if let Event::StatusChanged { status, .. } = e {
            let _ = tx.lock().unwrap().send(*status);
        }
    }));

    let rt = tokio::runtime::Runtime::new().unwrap();
    rt.block_on(async {
        let (client, _rx) = state.hub.register("test".into());
        let dir = tmp.path().join("proj");
        std::fs::create_dir_all(&dir).unwrap();
        let project = match state
            .handle(
                &client,
                1,
                Request::AddProject(lotse_proto::NewProject {
                    path: dir.to_string_lossy().into_owned(),
                    name: None,
                    color: None,
                    icon: None,
                }),
            )
            .await
        {
            Some(Response::Project(p)) => p,
            other => panic!("add_project: {other:?}"),
        };
        let session = match state
            .handle(
                &client,
                2,
                Request::CreateSession(NewSession {
                    project_id: project.id,
                    agent: AgentKind::ClaudeCode,
                    command: Some("/bin/sh".into()),
                    title: None,
                    worktree: None,
                    first_prompt: None,
                }),
            )
            .await
        {
            Some(Response::Session(s)) => s,
            other => panic!("create_session: {other:?}"),
        };
        state
            .handle(
                &client,
                3,
                Request::HookEvent {
                    agent: AgentKind::ClaudeCode,
                    session_id: Some(session.id),
                    payload: json!({"hook_event_name": "UserPromptSubmit"}),
                },
            )
            .await;
    });
    let saw_run = std::iter::from_fn(|| rx.recv_timeout(Duration::from_secs(5)).ok())
        .any(|status| status == Status::Run);
    assert!(saw_run, "no status_changed to run reached the observer");
    state.kill_all_sessions();
}
