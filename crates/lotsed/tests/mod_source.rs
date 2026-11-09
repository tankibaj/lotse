//! Beta M2: the Claude mod's side of the daemon, on an in-process `State` with a temp data dir (no
//! real agent, no real `~/.claude`): who may attach, that commands reach only the attached
//! connection, and that status lines move the session, its sub-agents and today's usage.

use lotse_proto::{
    AgentKind, ErrorCode, Event, Frame, Message, ModCommand, NewProject, NewSession, Project,
    Request, Response, Session, SessionId, SourceLayer, Status, read_frame,
};
use lotsed::config::Config;
use lotsed::db::Db;
use lotsed::hub::{Bytes, ClientTx};
use lotsed::state::State;
use serde_json::{Value, json};
use std::sync::Arc;
use tokio::sync::mpsc::UnboundedReceiver;

struct Conn {
    client: Arc<ClientTx>,
    rx: UnboundedReceiver<Bytes>,
}

impl Conn {
    /// Every event queued for this connection since the last call.
    fn events(&mut self) -> Vec<Event> {
        let mut out = Vec::new();
        while let Ok(bytes) = self.rx.try_recv() {
            if let Ok(Some(Frame::Control(Message::Event(e)))) = read_frame(&mut &bytes[..]) {
                out.push(e);
            }
        }
        out
    }
}

struct Local {
    state: Arc<State>,
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
        Self {
            state: State::new(cfg, Db::open_in_memory().unwrap()).unwrap(),
            rt: tokio::runtime::Runtime::new().unwrap(),
            tmp,
            next_req: 0,
        }
    }

    fn connect(&self, name: &str) -> Conn {
        let (client, rx) = self.state.hub.register(name.into());
        Conn { client, rx }
    }

    fn request(&mut self, conn: &Conn, request: Request) -> Response {
        self.next_req += 1;
        self.rt
            .block_on(self.state.handle(&conn.client, self.next_req, request))
            .expect("a response")
    }

    fn project(&mut self, conn: &Conn) -> Project {
        let dir = self.tmp.path().join("proj");
        std::fs::create_dir_all(&dir).unwrap();
        match self.request(
            conn,
            Request::AddProject(NewProject {
                path: dir.to_string_lossy().into_owned(),
                name: None,
                color: None,
                icon: None,
            }),
        ) {
            Response::Project(p) => p,
            other => panic!("add_project: {other:?}"),
        }
    }

    fn claude(&mut self, conn: &Conn, project: &Project) -> Session {
        match self.request(
            conn,
            Request::CreateSession(NewSession {
                project_id: project.id.clone(),
                agent: AgentKind::ClaudeCode,
                command: Some("/bin/sh".into()),
                title: None,
                worktree: None,
                first_prompt: None,
                parent_session: None,
                requested_by_agent: false,
            }),
        ) {
            Response::Session(s) => s,
            other => panic!("create_session: {other:?}"),
        }
    }

    fn send(&mut self, conn: &Conn, session: &SessionId, payload: Value) -> Response {
        self.request(
            conn,
            Request::HookEvent {
                agent: AgentKind::ClaudeCode,
                session_id: Some(session.clone()),
                payload,
            },
        )
    }

    /// A status line the way `lotse-hook mod-bridge` sends it.
    fn mod_line(&mut self, conn: &Conn, session: &SessionId, mut line: Value) -> Response {
        line["kind"] = "lotse_mod".into();
        line["lotse_source"] = "mod".into();
        self.send(conn, session, line)
    }

    fn session(&mut self, id: &SessionId) -> Session {
        let admin = self.connect("app");
        match self.request(&admin, Request::ListSessions { project_id: None }) {
            Response::Sessions(all) => all.into_iter().find(|s| &s.id == id).expect("session"),
            other => panic!("list_sessions: {other:?}"),
        }
    }

    fn usage(&mut self) -> lotse_proto::UsageToday {
        let admin = self.connect("app");
        match self.request(&admin, Request::GetSnapshot) {
            Response::Snapshot(s) => s.usage,
            other => panic!("get_snapshot: {other:?}"),
        }
    }
}

fn attach(l: &mut Local, conn: &Conn, id: &SessionId) -> Response {
    l.request(
        conn,
        Request::ModAttach {
            session_id: id.clone(),
        },
    )
}

#[test]
fn only_the_mod_client_attaches_and_only_to_a_known_session() {
    let mut l = Local::new();
    let app = l.connect("app");
    let project = l.project(&app);
    let s = l.claude(&app, &project);
    let (modc, other) = (l.connect("mod"), l.connect("hook"));

    assert!(matches!(attach(&mut l, &modc, &s.id), Response::Ok));
    let Response::Error(e) = attach(&mut l, &other, &s.id) else {
        panic!("another client attached");
    };
    assert_eq!(e.code, ErrorCode::InvalidRequest);
    let Response::Error(e) = attach(&mut l, &modc, &SessionId::new("s-nope")) else {
        panic!("attached to nothing");
    };
    assert_eq!(e.code, ErrorCode::NotFound);
}

#[test]
fn a_mod_command_reaches_only_the_connection_that_attached() {
    let mut l = Local::new();
    let app = l.connect("app");
    let project = l.project(&app);
    let (s1, s2) = (l.claude(&app, &project), l.claude(&app, &project));
    let (mut mod1, mut mod2) = (l.connect("mod"), l.connect("mod"));
    let mut bystander = l.connect("app");
    attach(&mut l, &mod1, &s1.id);
    attach(&mut l, &mod2, &s2.id);
    for conn in [&mut mod1, &mut mod2, &mut bystander] {
        conn.events();
    }

    assert!(l.state.send_mod_command(&s1.id, ModCommand::Ping));
    let heard = mod1.events();
    assert_eq!(
        heard,
        [Event::ModCommand {
            session_id: s1.id.clone(),
            command: ModCommand::Ping
        }]
    );
    assert!(mod2.events().is_empty(), "the mod of another session");
    assert!(
        bystander.events().is_empty(),
        "a client that did not attach"
    );

    // nobody attached to this one, and a connection that left is not written to
    let (s3, gone) = (l.claude(&app, &project), mod2.client.id);
    assert!(!l.state.send_mod_command(&s3.id, ModCommand::Ping));
    l.state.client_gone(gone);
    assert!(!l.state.send_mod_command(&s2.id, ModCommand::Ping));
}

#[test]
fn a_mod_line_from_any_other_client_is_refused() {
    let mut l = Local::new();
    let app = l.connect("app");
    let project = l.project(&app);
    let s = l.claude(&app, &project);
    let hook = l.connect("hook");
    let r = l.mod_line(
        &hook,
        &s.id,
        json!({"t": "perm", "agent_id": "a1", "tool": "Bash", "summary": "ls"}),
    );
    assert!(
        matches!(r, Response::Error(ref e) if e.code == ErrorCode::InvalidRequest),
        "{r:?}"
    );
    assert_ne!(l.session(&s.id).status, Status::Perm);
}

#[test]
fn a_mod_ask_is_perm_from_the_mod_and_ends_with_the_agents_next_tool_call() {
    let mut l = Local::new();
    let mut app = l.connect("app");
    let project = l.project(&app);
    let s = l.claude(&app, &project);
    let modc = l.connect("mod");
    let hook = l.connect("hook");
    l.send(&hook, &s.id, json!({"hook_event_name": "UserPromptSubmit"}));
    app.events();

    l.mod_line(
        &modc,
        &s.id,
        json!({"t": "perm", "agent_id": "a1", "tool": "Bash", "summary": "rm -rf build"}),
    );
    let now = l.session(&s.id);
    assert_eq!(
        (now.status, now.source),
        (Status::Perm, lotse_proto::DetectSource::Mod)
    );
    assert_eq!(
        now.perm.as_ref().map(|p| p.summary.as_str()),
        Some("rm -rf build")
    );
    assert!(now.perm.unwrap().id.is_some(), "a prompt id for Allow/Deny");
    assert!(app.events().iter().any(|e| matches!(
        e,
        Event::StatusChanged {
            status: Status::Perm,
            source: lotse_proto::DetectSource::Mod,
            ..
        }
    )));

    l.mod_line(
        &modc,
        &s.id,
        json!({"t": "tool", "agent_id": "a1", "tool": "Bash", "summary": "rm -rf build"}),
    );
    let now = l.session(&s.id);
    assert_eq!((now.status, now.perm), (Status::Run, None));
}

#[test]
fn a_heartbeat_puts_agents_context_and_rate_limits_into_the_state() {
    let mut l = Local::new();
    let mut app = l.connect("app");
    let project = l.project(&app);
    let s = l.claude(&app, &project);
    let modc = l.connect("mod");
    app.events();

    l.mod_line(
        &modc,
        &s.id,
        json!({
            "t": "heartbeat", "ts": 1, "context_pct": 42, "context_window": 200000,
            "rate_limits": [{"kind": "five_hour", "pct": 61, "resets_at": 1_800_000_000_000u64}],
            "agents": [{"id": "a1", "parent": null, "type": "Explore", "description": "map the repo", "status": "running"}],
        }),
    );
    let now = l.session(&s.id);
    assert_eq!(now.context_pct, Some(42));
    assert_eq!(now.source_layer, Some(SourceLayer::Mod));
    assert_eq!(now.sub_agents.len(), 1);
    assert_eq!(now.sub_agents[0].name, "map the repo");

    let events = app.events();
    assert!(
        events
            .iter()
            .any(|e| matches!(e, Event::SessionUpdated(u) if u.context_pct == Some(42)))
    );
    let usage = events
        .iter()
        .find_map(|e| match e {
            Event::UsageChanged(u) => Some(u.clone()),
            _ => None,
        })
        .expect("usage_changed");
    assert_eq!(usage.rate_limits.len(), 1);
    assert_eq!(
        (usage.rate_limits[0].kind.as_str(), usage.rate_limits[0].pct),
        ("five_hour", 61)
    );

    // the transcript refresh rebuilds the token totals and must keep the meter
    l.state.refresh_usage_at(1_800_000_000_000 - 3_600_000);
    assert_eq!(l.usage().rate_limits, usage.rate_limits);
}

#[test]
fn the_source_layer_follows_what_the_session_has_heard() {
    let mut l = Local::new();
    let app = l.connect("app");
    let project = l.project(&app);
    let s = l.claude(&app, &project);
    assert_eq!(l.session(&s.id).source_layer, None, "nothing heard yet");
    let hook = l.connect("hook");
    l.send(&hook, &s.id, json!({"hook_event_name": "UserPromptSubmit"}));
    assert_eq!(l.session(&s.id).source_layer, Some(SourceLayer::Hooks));
    let modc = l.connect("mod");
    l.mod_line(&modc, &s.id, json!({"t": "hello", "mod": 1}));
    assert_eq!(l.session(&s.id).source_layer, Some(SourceLayer::Mod));
}

#[test]
fn junk_numbers_cannot_break_the_state() {
    let mut l = Local::new();
    let app = l.connect("app");
    let project = l.project(&app);
    let s = l.claude(&app, &project);
    let modc = l.connect("mod");
    l.mod_line(
        &modc,
        &s.id,
        json!({"t": "heartbeat", "context_pct": 900, "rate_limits": [{"kind": "x", "pct": -3, "resets_at": "soon"}, 7, {"pct": 5}]}),
    );
    assert_eq!(l.session(&s.id).context_pct, Some(100), "clamped");
    assert!(
        l.usage().rate_limits.is_empty(),
        "entries without a valid pct or time are skipped"
    );
}
