//! Beta wave 0 on the real daemon: every new request is answered, a stale `prompt_id` is a
//! `conflict` that types nothing, and the hub observer sees events.
//! Temp data dir, temp HOME: nothing of the real `~/.claude` or Lotse data is touched.

mod common;

use common::{Client, Daemon};
use lotse_proto::{
    AgentKind, DiffBase, ErrorCode, Event, NewProject, NewSession, PermDecision, PermVia, Request,
    Response, Session, SessionId, Status,
};
use lotsed::config::Config;
use lotsed::db::Db;
use lotsed::hub::{ClientTx, PermDecisionRecord};
use lotsed::state::State;
use serde_json::json;
use std::sync::Arc;
use std::time::Duration;

fn conflict(r: &Response) -> bool {
    matches!(r, Response::Error(e) if e.code == ErrorCode::Conflict)
}

#[test]
fn get_diff_answers_for_both_bases() {
    let d = Daemon::start();
    let mut c = d.client();
    let session_id = SessionId::new("nope");
    let s = || session_id.clone();
    // every wave-1 request has landed (the last, export_transcript, with wp07), so none answers
    // `unavailable` any more: an unknown session is not_found
    let diff = |c: &mut Client, base| {
        c.request(Request::GetDiff {
            session_id: s(),
            base,
        })
    };
    for base in [DiffBase::Head, DiffBase::MergeBase] {
        let resp = diff(&mut c, base);
        assert!(
            matches!(&resp, Response::Error(e) if e.code == ErrorCode::NotFound),
            "{resp:?}"
        );
    }
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
        parent_session: None,
        requested_by_agent: false,
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

/// The daemon's `State` inside the test process (temp dirs, in-memory database), for what only the
/// in-process hub can show: observers.
struct Local {
    state: Arc<State>,
    client: Arc<ClientTx>,
    rt: tokio::runtime::Runtime,
    tmp: tempfile::TempDir,
    next_req: u64,
}

impl Local {
    fn new() -> Self {
        let tmp = tempfile::tempdir().unwrap();
        let home = tmp.path().join("home");
        std::fs::create_dir_all(&home).unwrap();
        let mut cfg = Config::new(tmp.path().join("data"), false);
        cfg.home = Some(home);
        let state = State::new(cfg, Db::open_in_memory().unwrap()).unwrap();
        let (client, _rx) = state.hub.register("test".into());
        Self {
            state,
            client,
            rt: tokio::runtime::Runtime::new().unwrap(),
            tmp,
            next_req: 0,
        }
    }

    fn request(&mut self, request: Request) -> Response {
        self.next_req += 1;
        let (state, client, id) = (&self.state, &self.client, self.next_req);
        self.rt
            .block_on(state.handle(client, id, request))
            .expect("a response")
    }

    fn agent_session(&mut self, command: &str) -> Session {
        let dir = self.tmp.path().join("proj");
        std::fs::create_dir_all(&dir).unwrap();
        let Response::Project(project) = self.request(Request::AddProject(NewProject {
            path: dir.to_string_lossy().into_owned(),
            name: None,
            color: None,
            icon: None,
        })) else {
            panic!("add_project")
        };
        match self.request(Request::CreateSession(NewSession {
            project_id: project.id,
            agent: AgentKind::ClaudeCode,
            command: Some(command.into()),
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

    fn hook(&mut self, s: &Session, payload: serde_json::Value) {
        let r = self.request(Request::HookEvent {
            agent: AgentKind::ClaudeCode,
            session_id: Some(s.id.clone()),
            payload,
        });
        assert!(matches!(r, Response::Ok), "{r:?}");
    }

    fn open_prompt_id(&mut self, s: &Session) -> Option<String> {
        let Response::Sessions(list) = self.request(Request::ListSessions { project_id: None })
        else {
            panic!("list_sessions")
        };
        let found = list.into_iter().find(|x| x.id == s.id).unwrap();
        found.perm.and_then(|p| p.id)
    }
}

impl Drop for Local {
    fn drop(&mut self) {
        self.state.kill_all_sessions();
    }
}

#[test]
fn a_hub_observer_sees_status_changed() {
    let mut local = Local::new();
    let (tx, rx) = std::sync::mpsc::channel();
    let tx = std::sync::Mutex::new(tx);
    local.state.hub.observe(Box::new(move |e: &Event| {
        if let Event::StatusChanged { status, .. } = e {
            let _ = tx.lock().unwrap().send(*status);
        }
    }));
    let s = local.agent_session("/bin/sh");
    local.hook(&s, json!({"hook_event_name": "UserPromptSubmit"}));
    let saw_run = std::iter::from_fn(|| rx.recv_timeout(Duration::from_secs(5)).ok())
        .any(|status| status == Status::Run);
    assert!(saw_run, "no status_changed to run reached the observer");
}

/// A pane that paints a permission prompt and waits for a key, inside a `Local` session.
fn prompt_script(local: &Local) -> String {
    let dir = local.tmp.path().join("script");
    std::fs::create_dir_all(&dir).unwrap();
    common::script(
        &dir,
        "agent.sh",
        "stty -icanon -echo min 1\n\
         printf ' Do you want to proceed?\\n ❯ 1. Yes\\n   2. No\\n'\n\
         dd bs=1 count=1 >/dev/null 2>&1\nsleep 5",
    );
    dir.join("agent.sh").to_string_lossy().into_owned()
}

#[test]
fn a_decision_observer_sees_the_answer_and_its_via_but_not_a_refused_one() {
    let mut local = Local::new();
    let (tx, rx) = std::sync::mpsc::channel();
    let tx = std::sync::Mutex::new(tx);
    local
        .state
        .hub
        .observe_decisions(Box::new(move |r: &PermDecisionRecord| {
            let _ = tx.lock().unwrap().send(r.clone());
        }));
    let script = prompt_script(&local);
    let s = local.agent_session(&script);
    local.hook(
        &s,
        json!({"hook_event_name": "PermissionRequest", "tool_name": "Bash",
            "tool_input": {"command": "ls"}}),
    );
    let id = local.open_prompt_id(&s).expect("a prompt id");
    let answer = |local: &mut Local, prompt_id: &str| {
        local.request(Request::ResolvePermission {
            session_id: s.id.clone(),
            decision: PermDecision::Allow,
            via: Some(PermVia::Notification),
            prompt_id: Some(prompt_id.into()),
        })
    };

    // a stale banner is refused and nothing is published
    assert!(conflict(&answer(&mut local, "an-older-prompt")));
    assert!(rx.try_recv().is_err(), "a refused answer was published");

    // the prompt is read off the screen a moment after it is drawn: Conflict until then
    let start = std::time::Instant::now();
    while !matches!(answer(&mut local, &id), Response::Ok) {
        assert!(
            start.elapsed() < Duration::from_secs(10),
            "never answerable"
        );
        assert!(rx.try_recv().is_err(), "a refused answer was published");
        std::thread::sleep(Duration::from_millis(50));
    }
    assert_eq!(
        rx.try_recv().expect("the answer was published"),
        PermDecisionRecord {
            session_id: s.id.clone(),
            prompt_id: Some(id),
            decision: PermDecision::Allow,
            via: Some(PermVia::Notification),
        }
    );
    assert!(rx.try_recv().is_err(), "published once");
}

#[test]
fn a_repeated_report_of_the_same_request_keeps_one_prompt_id() {
    let d = Daemon::start();
    let mut c = d.client();
    let dir = tempfile::tempdir().unwrap();
    common::script(
        dir.path(),
        "agent.sh",
        "stty -icanon -echo min 1\n\
         printf ' Do you want to proceed?\\n ❯ 1. Yes\\n   2. No\\n'\n\
         sleep 1\nprintf 'REDRAWN\\n'\n\
         printf 'BYTES:%s\\n' \"$(dd bs=1 count=1 2>/dev/null | od -An -tx1 | tr -d ' \\n')\"\nsleep 5",
    );
    let s = agent_session(&mut c, &d, &dir.path().join("agent.sh").to_string_lossy());
    c.attach(&s.id, 80, 24);
    assert!(c.wait_output(&s.id, "2. No"));
    let open_id = |c: &mut Client| {
        let Response::Sessions(list) = c.request(Request::ListSessions { project_id: None }) else {
            panic!("list_sessions")
        };
        list[0].perm.as_ref().and_then(|p| p.id.clone())
    };

    let first = ask_permission(&mut c, &s, "ls");
    // the agent reports the same request again (a hook retry)
    ask_again(&mut c, &s, "ls");
    assert_eq!(open_id(&mut c), Some(first.clone()), "hook re-report");
    // a sub-agent starts while the request is open: the engine reports the request again
    hook(
        &mut c,
        &s,
        json!({"hook_event_name": "SubagentStart", "agent_id": "sub-1", "agent_type": "explorer"}),
    );
    c.wait_event(|e| matches!(e, Event::SubAgentsChanged { .. }));
    assert_eq!(
        open_id(&mut c),
        Some(first.clone()),
        "update with the same request"
    );
    // the screen is sampled again after more output
    assert!(c.wait_output(&s.id, "REDRAWN"));
    c.drain(Duration::from_millis(500));
    assert_eq!(open_id(&mut c), Some(first.clone()), "screen re-report");

    assert!(matches!(resolve(&mut c, &s, Some(&first)), Response::Ok));
    assert!(c.wait_output(&s.id, "BYTES:31"));
}

/// The same `PermissionRequest` again, without waiting for a status change (there is none).
fn ask_again(c: &mut Client, s: &Session, command: &str) {
    hook(
        c,
        s,
        json!({"hook_event_name": "PermissionRequest", "tool_name": "Bash",
            "tool_input": {"command": command}}),
    );
}
