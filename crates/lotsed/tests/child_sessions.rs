//! Child sessions on the real daemon binary: what an agent's `lotse-hook mcp` connection
//! (`Hello.client` = `mcp:<session>`) may do, the approval card, and the limits.
//! The agent is a fake `claude` script (`cat`), the data dir and HOME are temp dirs.

mod common;

use common::{Client, Daemon, script};
use lotse_proto::{
    AgentKind, ErrorCode, Event, NewSession, PermDecision, Project, Request, Response, Session,
    SessionId, Status,
};
use std::time::Duration;

struct Rig {
    d: Daemon,
    app: Client,
    project: Project,
}

fn rig() -> Rig {
    let bin = tempfile::Builder::new().prefix("lb").tempdir().unwrap();
    script(bin.path(), "claude", "exec cat");
    let d = Daemon::start_with_path(Some(bin.path()));
    // the daemon resolved nothing yet: keep the fake binary alive for the whole test
    std::mem::forget(bin);
    let mut app = d.client();
    let project = app.add_project(&d.project_dir("praxis"));
    Rig { d, app, project }
}

fn agent_session(app: &mut Client, project: &Project) -> Session {
    match app.request(Request::CreateSession(NewSession {
        project_id: project.id.clone(),
        agent: AgentKind::ClaudeCode,
        command: None,
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

fn mcp(d: &Daemon, session: &SessionId) -> Client {
    Client::connect_as(&d.socket(), &d.token(), &format!("mcp:{session}"))
}

fn start_request(project: &Project, prompt: &str) -> Request {
    Request::CreateSession(NewSession {
        project_id: project.id.clone(),
        agent: AgentKind::ClaudeCode,
        command: None,
        title: Some("Fix the flaky test".into()),
        worktree: None,
        first_prompt: Some(prompt.into()),
        parent_session: None,
        requested_by_agent: false,
    })
}

fn error_code(r: Response) -> ErrorCode {
    match r {
        Response::Error(e) => e.code,
        other => panic!("expected an error, got {other:?}"),
    }
}

fn card(app: &mut Client) -> (String, Event) {
    let e = app.wait_event(|e| matches!(e, Event::SessionStartRequested { .. }));
    let Event::SessionStartRequested { id, .. } = &e else {
        unreachable!()
    };
    (id.clone(), e)
}

/// The agent asks, the app answers `approve`; the agent's answer.
fn ask_and_answer(r: &mut Rig, agent: &mut Client, request: Request, approve: bool) -> Response {
    let req = agent.send(request);
    let (id, _) = card(&mut r.app);
    r.app
        .expect_ok(Request::ResolveSessionStart { id, approve });
    agent.response(req)
}

fn child_of(r: &mut Rig, parent: &Session) -> Session {
    let mut agent = mcp(&r.d, &parent.id);
    let request = start_request(&r.project, "work");
    match ask_and_answer(r, &mut agent, request, true) {
        Response::Session(s) => s,
        other => panic!("start: {other:?}"),
    }
}

fn hook_stop(app: &mut Client, s: &Session) {
    app.expect_ok(Request::HookEvent {
        agent: AgentKind::ClaudeCode,
        session_id: Some(s.id.clone()),
        payload: serde_json::json!({"hook_event_name": "Stop"}),
    });
    app.wait_event(|e| {
        matches!(
            e,
            Event::StatusChanged {
                status: Status::Done,
                ..
            }
        )
    });
}

fn sessions(app: &mut Client) -> Vec<Session> {
    match app.request(Request::ListSessions { project_id: None }) {
        Response::Sessions(s) => s,
        other => panic!("list: {other:?}"),
    }
}

#[test]
fn a_bare_mcp_client_has_no_session_and_may_only_ping() {
    let r = rig();
    let mut bare = Client::connect_as(&r.d.socket(), &r.d.token(), "mcp");
    assert!(matches!(bare.request(Request::Ping), Response::Pong));
    for req in [
        Request::ListSessions { project_id: None },
        start_request(&r.project, "x"),
        Request::GetSnapshot,
    ] {
        assert_eq!(error_code(bare.request(req)), ErrorCode::Unauthorized);
    }
}

#[test]
fn an_mcp_client_never_sees_pushed_events_or_other_sessions_prompts() {
    let mut r = rig();
    let parent = agent_session(&mut r.app, &r.project);
    let mut agent = mcp(&r.d, &parent.id);
    assert!(matches!(agent.request(Request::Ping), Response::Pong));
    // another start request and a new session happen: only the app hears of them
    let mut other = mcp(&r.d, &parent.id);
    other.send(start_request(&r.project, "secret instructions"));
    card(&mut r.app);
    agent_session(&mut r.app, &r.project);
    agent.drain(Duration::from_millis(400));
    assert!(agent.events.is_empty(), "{:?}", agent.events);
}

#[test]
fn a_start_blocks_until_the_app_approves_then_the_child_runs_under_its_parent() {
    let mut r = rig();
    let parent = agent_session(&mut r.app, &r.project);
    let mut agent = mcp(&r.d, &parent.id);
    let req = agent.send(start_request(
        &r.project,
        "Fix the flaky test in e2e/patients",
    ));

    let (card_id, event) = card(&mut r.app);
    let Event::SessionStartRequested {
        parent_session,
        project_id,
        agent: kind,
        title,
        prompt,
        worktree,
        ..
    } = event
    else {
        unreachable!()
    };
    assert_eq!(parent_session, parent.id);
    assert_eq!(project_id, r.project.id);
    assert_eq!(kind, AgentKind::ClaudeCode);
    assert_eq!(title.as_deref(), Some("Fix the flaky test"));
    assert_eq!(
        prompt, "Fix the flaky test in e2e/patients",
        "the full prompt"
    );
    assert_eq!(worktree, None);
    assert_eq!(
        sessions(&mut r.app).len(),
        1,
        "nothing starts before the answer"
    );

    r.app.expect_ok(Request::ResolveSessionStart {
        id: card_id.clone(),
        approve: true,
    });
    let Response::Session(child) = agent.response(req) else {
        panic!("the start was approved");
    };
    assert_eq!(child.parent_session.as_ref(), Some(&parent.id));
    assert_eq!(child.agent, AgentKind::ClaudeCode);
    assert_eq!(child.title, "Fix the flaky test");
    let created = r
        .app
        .wait_event(|e| matches!(e, Event::SessionCreated(s) if s.id == child.id));
    let Event::SessionCreated(created) = created else {
        unreachable!()
    };
    assert_eq!(created.parent_session.as_ref(), Some(&parent.id));
    // a card answers once
    assert_eq!(
        error_code(r.app.request(Request::ResolveSessionStart {
            id: card_id,
            approve: true
        })),
        ErrorCode::NotFound
    );
}

#[test]
fn an_agents_first_prompt_is_shown_and_typed_as_plain_lines_never_as_keystrokes() {
    let mut r = rig();
    let parent = agent_session(&mut r.app, &r.project);
    let mut agent = mcp(&r.d, &parent.id);
    // Shift+Tab (ESC [ Z) would switch Claude's permission mode if it reached the pane
    let req = agent.send(start_request(&r.project, "line one\nline two \u{1b}[Z end"));
    let (id, event) = card(&mut r.app);
    let Event::SessionStartRequested { prompt, .. } = event else {
        unreachable!()
    };
    assert_eq!(
        prompt, "line one\nline two [Z end",
        "the card shows what will be typed"
    );
    r.app
        .expect_ok(Request::ResolveSessionStart { id, approve: true });
    let Response::Session(kid) = agent.response(req) else {
        panic!("started");
    };
    r.app.attach(&kid.id, 100, 30);
    assert!(r.app.wait_output(&kid.id, "line two [Z end"));
    let out = r.app.output_text(&kid.id);
    assert!(out.contains("line one"), "{out:?}");
    assert!(!out.contains("^[[Z"), "an ESC reached the pane: {out:?}");
}

#[test]
fn a_denied_start_answers_conflict_and_starts_nothing() {
    let mut r = rig();
    let parent = agent_session(&mut r.app, &r.project);
    let mut agent = mcp(&r.d, &parent.id);
    let request = start_request(&r.project, "x");
    let answer = ask_and_answer(&mut r, &mut agent, request, false);
    assert_eq!(error_code(answer), ErrorCode::Conflict);
    assert_eq!(sessions(&mut r.app).len(), 1);
}

#[test]
fn an_agent_cannot_approve_its_own_start_and_an_unknown_card_is_not_found() {
    let mut r = rig();
    let parent = agent_session(&mut r.app, &r.project);
    let mut agent = mcp(&r.d, &parent.id);
    let mut helper = mcp(&r.d, &parent.id);
    let req = agent.send(start_request(&r.project, "x"));
    let (id, _) = card(&mut r.app);
    assert_eq!(
        error_code(helper.request(Request::ResolveSessionStart {
            id: id.clone(),
            approve: true
        })),
        ErrorCode::Unauthorized
    );
    assert_eq!(sessions(&mut r.app).len(), 1, "still nothing started");
    assert_eq!(
        error_code(r.app.request(Request::ResolveSessionStart {
            id: "r-nope".into(),
            approve: true
        })),
        ErrorCode::NotFound
    );
    r.app
        .expect_ok(Request::ResolveSessionStart { id, approve: true });
    assert!(matches!(agent.response(req), Response::Session(_)));
}

#[test]
fn bad_starts_are_refused_before_any_card() {
    let mut r = rig();
    let parent = agent_session(&mut r.app, &r.project);
    let mut agent = mcp(&r.d, &parent.id);
    let mut with = |f: &dyn Fn(&mut NewSession)| {
        let Request::CreateSession(mut ns) = start_request(&r.project, "x") else {
            unreachable!()
        };
        f(&mut ns);
        error_code(agent.request(Request::CreateSession(ns)))
    };
    assert_eq!(
        with(&|ns| ns.command = Some("rm -rf ~".into())),
        ErrorCode::InvalidRequest
    );
    assert_eq!(
        with(&|ns| ns.agent = AgentKind::Terminal),
        ErrorCode::InvalidRequest
    );
    assert_eq!(
        with(&|ns| ns.first_prompt = None),
        ErrorCode::InvalidRequest
    );
    assert_eq!(
        with(&|ns| ns.parent_session = Some(SessionId::new("s-other"))),
        ErrorCode::Unauthorized
    );
    assert_eq!(
        with(&|ns| ns.project_id = lotse_proto::ProjectId::new("nope")),
        ErrorCode::NotFound
    );
    r.app.drain(Duration::from_millis(300));
    assert!(
        !r.app
            .events
            .iter()
            .any(|e| matches!(e, Event::SessionStartRequested { .. })),
        "no card for a refused start"
    );
}

#[test]
fn a_session_one_two_levels_down_cannot_start_another() {
    let mut r = rig();
    let root = agent_session(&mut r.app, &r.project);
    let kid = child_of(&mut r, &root);
    let grandkid = child_of(&mut r, &kid);
    assert_eq!(grandkid.parent_session.as_ref(), Some(&kid.id));
    let mut deepest = mcp(&r.d, &grandkid.id);
    assert_eq!(
        error_code(deepest.request(start_request(&r.project, "x"))),
        ErrorCode::Conflict
    );
}

#[test]
fn closing_the_parent_detaches_its_children_and_withdraws_its_open_cards() {
    let mut r = rig();
    let parent = agent_session(&mut r.app, &r.project);
    let kid = child_of(&mut r, &parent);
    assert_eq!(kid.parent_session.as_ref(), Some(&parent.id));
    // an open card of the parent
    let mut agent = mcp(&r.d, &parent.id);
    let pending = agent.send(start_request(&r.project, "second"));
    let (card_id, _) = card(&mut r.app);
    r.app.expect_ok(Request::CloseSession {
        session_id: parent.id.clone(),
    });
    let e = r
        .app
        .wait_event(|e| matches!(e, Event::SessionStartWithdrawn { .. }));
    assert!(matches!(e, Event::SessionStartWithdrawn { id } if id == card_id));
    assert_eq!(error_code(agent.response(pending)), ErrorCode::Unavailable);
    let e = r.app.wait_event(
        |e| matches!(e, Event::SessionUpdated(s) if s.id == kid.id && s.parent_session.is_none()),
    );
    assert!(matches!(e, Event::SessionUpdated(_)));
    let kid_now = sessions(&mut r.app)
        .into_iter()
        .find(|s| s.id == kid.id)
        .unwrap();
    assert_eq!(kid_now.parent_session, None, "a top-level session now");
}

#[test]
fn a_card_that_timed_out_or_whose_agent_left_is_withdrawn_for_the_app() {
    let mut r = rig();
    let parent = agent_session(&mut r.app, &r.project);
    let mut agent = mcp(&r.d, &parent.id);
    agent.send(start_request(&r.project, "x"));
    let (card_id, _) = card(&mut r.app);
    agent.hang_up();
    let e = r
        .app
        .wait_event(|e| matches!(e, Event::SessionStartWithdrawn { .. }));
    assert!(matches!(e, Event::SessionStartWithdrawn { id } if id == card_id));
}

#[test]
fn with_the_toggle_on_a_start_in_the_parents_project_needs_no_card() {
    let mut r = rig();
    let parent = agent_session(&mut r.app, &r.project);
    let Response::Settings(mut s) = r.app.request(Request::GetSettings) else {
        panic!("settings")
    };
    assert!(!s.agents_start_sessions_without_asking, "off by default");
    s.agents_start_sessions_without_asking = true;
    assert!(matches!(
        r.app.request(Request::UpdateSettings { settings: s }),
        Response::Settings(_)
    ));
    let mut agent = mcp(&r.d, &parent.id);
    let Response::Session(kid) = agent.request(start_request(&r.project, "go")) else {
        panic!("started without a card")
    };
    assert_eq!(kid.parent_session.as_ref(), Some(&parent.id));
    r.app.drain(Duration::from_millis(300));
    assert!(
        !r.app
            .events
            .iter()
            .any(|e| matches!(e, Event::SessionStartRequested { .. })),
        "no card"
    );
}

#[test]
fn at_most_five_live_children() {
    let mut r = rig();
    let parent = agent_session(&mut r.app, &r.project);
    for _ in 0..5 {
        child_of(&mut r, &parent);
    }
    let mut agent = mcp(&r.d, &parent.id);
    assert_eq!(
        error_code(agent.request(start_request(&r.project, "one too many"))),
        ErrorCode::Conflict
    );
    // another session of the same project still has its own five
    let other = agent_session(&mut r.app, &r.project);
    child_of(&mut r, &other);
}

#[test]
fn list_shows_the_callers_project_and_relatives_only() {
    let mut r = rig();
    let other_project = r.app.add_project(&r.d.project_dir("elsewhere"));
    let parent = agent_session(&mut r.app, &r.project);
    let mate = agent_session(&mut r.app, &r.project);
    let stranger = agent_session(&mut r.app, &other_project);
    let kid = child_of(&mut r, &parent);
    let mut agent = mcp(&r.d, &parent.id);
    let Response::Sessions(list) = agent.request(Request::ListSessions { project_id: None }) else {
        panic!("sessions");
    };
    let mut ids: Vec<_> = list.iter().map(|s| s.id.to_string()).collect();
    ids.sort();
    let mut want = vec![
        parent.id.to_string(),
        mate.id.to_string(),
        kid.id.to_string(),
    ];
    want.sort();
    assert_eq!(ids, want);
    assert!(!ids.contains(&stranger.id.to_string()));
}

#[test]
fn send_message_reaches_an_idle_child_as_plain_text_and_never_a_busy_one() {
    let mut r = rig();
    let parent = agent_session(&mut r.app, &r.project);
    let kid = child_of(&mut r, &parent);
    let mut agent = mcp(&r.d, &parent.id);
    let send = |text: &str| Request::SendInput {
        session_id: kid.id.clone(),
        text: text.into(),
    };
    // a new agent session has no signal yet: not idle
    assert_eq!(error_code(agent.request(send("hi"))), ErrorCode::Conflict);

    hook_stop(&mut r.app, &kid);
    r.app.attach(&kid.id, 100, 30);
    agent.expect_ok(send("hello kid \u{1b}[201~ rm -rf ~"));
    assert!(r.app.wait_output(&kid.id, "hello kid [201~ rm -rf ~"));
    assert!(
        !r.app.output_text(&kid.id).contains("kid ^[[201~"),
        "an ESC from the message must not reach the pane"
    );
    // a permission prompt makes it not idle again
    r.app.expect_ok(Request::HookEvent {
        agent: AgentKind::ClaudeCode,
        session_id: Some(kid.id.clone()),
        payload: serde_json::json!({
            "hook_event_name": "PermissionRequest",
            "tool_name": "Bash",
            "tool_input": {"command": "ls"}
        }),
    });
    r.app.wait_event(|e| {
        matches!(
            e,
            Event::StatusChanged {
                status: Status::Perm,
                ..
            }
        )
    });
    assert_eq!(error_code(agent.request(send("yes"))), ErrorCode::Conflict);
}

#[test]
fn send_message_only_goes_to_a_child_or_the_parent() {
    let mut r = rig();
    let parent = agent_session(&mut r.app, &r.project);
    let stranger = agent_session(&mut r.app, &r.project);
    hook_stop(&mut r.app, &stranger);
    let mut agent = mcp(&r.d, &parent.id);
    assert_eq!(
        error_code(agent.request(Request::SendInput {
            session_id: stranger.id.clone(),
            text: "hi".into()
        })),
        ErrorCode::Unauthorized
    );
    // the child talks to its parent, once the parent is idle
    let kid = child_of(&mut r, &parent);
    let mut child_side = mcp(&r.d, &kid.id);
    let to_parent = Request::SendInput {
        session_id: parent.id.clone(),
        text: "done".into(),
    };
    assert_eq!(
        error_code(child_side.request(to_parent.clone())),
        ErrorCode::Conflict
    );
    hook_stop(&mut r.app, &parent);
    child_side.expect_ok(to_parent);
}

#[test]
fn close_session_takes_own_children_only() {
    let mut r = rig();
    let parent = agent_session(&mut r.app, &r.project);
    let kid = child_of(&mut r, &parent);
    let mut child_side = mcp(&r.d, &kid.id);
    assert_eq!(
        error_code(child_side.request(Request::CloseSession {
            session_id: parent.id.clone()
        })),
        ErrorCode::Unauthorized
    );
    let mut agent = mcp(&r.d, &parent.id);
    agent.expect_ok(Request::CloseSession {
        session_id: kid.id.clone(),
    });
    r.app
        .wait_event(|e| matches!(e, Event::SessionClosed { session_id } if *session_id == kid.id));
}

#[test]
fn transcript_is_for_relatives_only() {
    let mut r = rig();
    let parent = agent_session(&mut r.app, &r.project);
    let stranger = agent_session(&mut r.app, &r.project);
    let kid = child_of(&mut r, &parent);
    let mut agent = mcp(&r.d, &parent.id);
    let ask = |id: &SessionId| Request::GetTranscript {
        session_id: id.clone(),
        limit: Some(500),
    };
    assert!(matches!(
        agent.request(ask(&kid.id)),
        Response::Transcript(_)
    ));
    assert_eq!(
        error_code(agent.request(ask(&stranger.id))),
        ErrorCode::Unauthorized
    );
}

#[test]
fn everything_else_is_refused_for_an_mcp_client() {
    let mut r = rig();
    let parent = agent_session(&mut r.app, &r.project);
    let mut agent = mcp(&r.d, &parent.id);
    for req in [
        Request::GetSnapshot,
        Request::GetSettings,
        Request::ResolvePermission {
            session_id: parent.id.clone(),
            decision: PermDecision::Allow,
            via: None,
            prompt_id: None,
        },
        Request::Attach {
            session_id: parent.id.clone(),
            cols: 80,
            rows: 24,
        },
        Request::RemoveProject {
            project_id: r.project.id.clone(),
        },
        Request::Shutdown,
    ] {
        let method = req.method();
        assert_eq!(
            error_code(agent.request(req)),
            ErrorCode::Unauthorized,
            "{method}"
        );
    }
    assert_eq!(sessions(&mut r.app).len(), 1, "the daemon is still up");
}

/// A workspace binary next to the test executable (`target/<profile>/<name>`).
fn locate(name: &str) -> Option<std::path::PathBuf> {
    let exe = std::env::current_exe().ok()?;
    let p = exe.parent()?.parent()?.join(name);
    p.is_file().then_some(p)
}

/// The real `lotse-hook mcp` process on stdio, the way an agent starts it: the environment the
/// daemon exports into a pane, no token in it.
struct McpProcess {
    child: std::process::Child,
    stdin: std::process::ChildStdin,
    stdout: std::io::BufReader<std::process::ChildStdout>,
    next_id: u64,
}

impl McpProcess {
    fn spawn(d: &Daemon, session: &Session) -> Self {
        use std::process::{Command, Stdio};
        let bin = locate("lotse-hook")
            .expect("lotse-hook binary not built: run `cargo build -p lotse-hook`");
        let mut child = Command::new(bin)
            .arg("mcp")
            .env_clear()
            .env("HOME", d.home.as_os_str())
            .env("LOTSE_SESSION_ID", session.id.as_str())
            .env("LOTSE_PROJECT_ID", session.project_id.as_str())
            .env("LOTSE_SOCKET", d.socket())
            .env("LOTSE_TOKEN_FILE", d.token_path())
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        let stdin = child.stdin.take().unwrap();
        let stdout = std::io::BufReader::new(child.stdout.take().unwrap());
        Self {
            child,
            stdin,
            stdout,
            next_id: 0,
        }
    }

    fn send_call(&mut self, tool: &str, args: serde_json::Value) {
        use std::io::Write;
        self.next_id += 1;
        let line = serde_json::json!({"jsonrpc":"2.0","id":self.next_id,"method":"tools/call","params":{"name":tool,"arguments":args}});
        writeln!(self.stdin, "{line}").unwrap();
    }

    /// (isError, the result's JSON) of the answer to the last call.
    fn answer(&mut self) -> (bool, serde_json::Value) {
        use std::io::BufRead;
        let mut line = String::new();
        self.stdout.read_line(&mut line).unwrap();
        let v: serde_json::Value = serde_json::from_str(&line).unwrap();
        assert_eq!(v["id"], self.next_id, "{v}");
        let text = v["result"]["content"][0]["text"]
            .as_str()
            .unwrap_or_default();
        let parsed = serde_json::from_str(text).unwrap_or(serde_json::json!(text));
        (v["result"]["isError"] == true, parsed)
    }

    fn call(&mut self, tool: &str, args: serde_json::Value) -> (bool, serde_json::Value) {
        self.send_call(tool, args);
        self.answer()
    }
}

impl Drop for McpProcess {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

#[test]
fn the_real_mcp_server_runs_a_whole_child_session_against_the_real_daemon() {
    let mut r = rig();
    let parent = agent_session(&mut r.app, &r.project);
    let mut mcp = McpProcess::spawn(&r.d, &parent);

    // start: the tool call waits for the card
    mcp.send_call(
        "start_session",
        serde_json::json!({"title":"Count the tests","prompt":"count the tests\nreport the number"}),
    );
    let (card_id, event) = card(&mut r.app);
    let Event::SessionStartRequested {
        prompt,
        parent_session,
        ..
    } = event
    else {
        unreachable!()
    };
    assert_eq!(prompt, "count the tests\nreport the number");
    assert_eq!(parent_session, parent.id);
    r.app.expect_ok(Request::ResolveSessionStart {
        id: card_id,
        approve: true,
    });
    let (is_error, started) = mcp.answer();
    assert!(!is_error, "{started}");
    let kid_id = started["session_id"].as_str().unwrap().to_owned();
    assert_eq!(started["parent"], parent.id.as_str());

    // it shows up under its parent
    let (_, listed) = mcp.call("list_sessions", serde_json::json!({}));
    assert_eq!(listed["sessions"][0]["id"], kid_id.as_str());

    // wait: nothing reports yet; then the child finishes
    let (_, early) = mcp.call(
        "wait_session",
        serde_json::json!({"session_id": kid_id, "timeout_s": 0}),
    );
    assert_eq!(early["timed_out"], true);
    let kid = Session {
        id: SessionId::new(kid_id.clone()),
        ..parent.clone()
    };
    hook_stop(&mut r.app, &kid);
    let (_, done) = mcp.call(
        "wait_session",
        serde_json::json!({"session_id": kid_id, "timeout_s": 20}),
    );
    assert_eq!(done["status"], "done");

    // message the idle child; it reaches the pane
    r.app.attach(&kid.id, 100, 30);
    let (is_error, _) = mcp.call(
        "send_message",
        serde_json::json!({"session_id": kid_id, "text": "thanks, now stop"}),
    );
    assert!(!is_error);
    assert!(r.app.wait_output(&kid.id, "thanks, now stop"));

    // an unknown argument never gets as far as the daemon
    let (is_error, text) = mcp.call(
        "close_session",
        serde_json::json!({"session_id": kid_id, "command": "rm -rf ~"}),
    );
    assert!(is_error, "{text}");
    assert_eq!(sessions(&mut r.app).len(), 2);

    let (is_error, _) = mcp.call("close_session", serde_json::json!({"session_id": kid_id}));
    assert!(!is_error);
    r.app.wait_event(
        |e| matches!(e, Event::SessionClosed { session_id } if session_id.as_str() == kid_id),
    );
    let (_, after) = mcp.call("wait_session", serde_json::json!({"session_id": kid_id}));
    assert_eq!(after["status"], "closed");
}

#[test]
fn only_the_daemon_marks_a_session_as_requested_by_an_agent() {
    let mut r = rig();
    let parent = agent_session(&mut r.app, &r.project);
    let new = |parent: Option<&SessionId>, requested: bool| {
        Request::CreateSession(NewSession {
            project_id: r.project.id.clone(),
            agent: AgentKind::ClaudeCode,
            command: None,
            title: None,
            worktree: None,
            first_prompt: None,
            parent_session: parent.cloned(),
            requested_by_agent: requested,
        })
    };
    // an app client cannot claim an agent asked
    assert_eq!(
        error_code(r.app.request(new(None, true))),
        ErrorCode::InvalidRequest
    );
    // an unknown parent is refused, a known one is kept
    assert_eq!(
        error_code(r.app.request(new(Some(&SessionId::new("s-nope")), false))),
        ErrorCode::NotFound
    );
    let Response::Session(s) = r.app.request(new(Some(&parent.id), false)) else {
        panic!("a child started by the user");
    };
    assert_eq!(s.parent_session.as_ref(), Some(&parent.id));
}
