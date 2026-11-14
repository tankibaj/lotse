//! Beta M4: the two-way half of the Claude mod, on an in-process `State` with a temp data dir (no real
//! agent): a sidebar Allow/Deny reaches the mod while its `PermissionRequest` hook is still held (the
//! hook is cut at 10 s, so the window is 8 s) and takes the keystroke path otherwise; a composer send
//! goes to the mod, and is typed when the mod cannot take it. Only the app may decide.

use lotse_proto::{
    AgentKind, ErrorCode, Event, Frame, Message, ModCommand, NewProject, NewSession, PermDecision,
    Project, Request, Response, Session, SessionId, Status, read_frame,
};
use lotsed::config::Config;
use lotsed::db::Db;
use lotsed::hub::{Bytes, ClientTx};
use lotsed::state::State;
use serde_json::{Value, json};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::mpsc::UnboundedReceiver;

/// How the composer marks a send for the mod (`app/src/composer/send.ts` has the same string).
const SUBMIT_MARKER: &str = "\u{0}lotse-submit\u{0}";

struct Conn {
    client: Arc<ClientTx>,
    rx: UnboundedReceiver<Bytes>,
}

impl Conn {
    fn frames(&mut self) -> Vec<Frame> {
        let mut out = Vec::new();
        while let Ok(bytes) = self.rx.try_recv() {
            if let Ok(Some(frame)) = read_frame(&mut &bytes[..]) {
                out.push(frame);
            }
        }
        out
    }

    /// The commands queued for this connection since the last call.
    fn commands(&mut self) -> Vec<ModCommand> {
        self.frames()
            .into_iter()
            .filter_map(|f| match f {
                Frame::Control(Message::Event(Event::ModCommand { command, .. })) => Some(command),
                _ => None,
            })
            .collect()
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

    fn claude(&mut self, conn: &Conn, project: &Project, command: &str) -> Session {
        match self.request(
            conn,
            Request::CreateSession(NewSession {
                project_id: project.id.clone(),
                agent: AgentKind::ClaudeCode,
                command: Some(command.into()),
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

    fn hook(&mut self, conn: &Conn, session: &SessionId, payload: Value) -> Response {
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
    fn mod_line(&mut self, conn: &Conn, session: &SessionId, mut line: Value) {
        line["kind"] = "lotse_mod".into();
        line["lotse_source"] = "mod".into();
        assert!(matches!(self.hook(conn, session, line), Response::Ok));
    }

    fn session(&mut self, id: &SessionId) -> Session {
        let app = self.connect("app");
        match self.request(&app, Request::ListSessions { project_id: None }) {
            Response::Sessions(all) => all.into_iter().find(|s| &s.id == id).expect("session"),
            other => panic!("list_sessions: {other:?}"),
        }
    }

    fn resolve(&mut self, conn: &Conn, session: &SessionId, decision: PermDecision) -> Response {
        self.request(
            conn,
            Request::ResolvePermission {
                session_id: session.clone(),
                decision,
                via: None,
                prompt_id: None,
            },
        )
    }
}

/// A Claude session at a `Bash` permission prompt, its mod attached (`modc`) and holding the hook.
struct AtPrompt {
    l: Local,
    app: Conn,
    modc: Conn,
    session: Session,
}

fn at_prompt(hold: Option<&str>) -> AtPrompt {
    let mut l = Local::new();
    let app = l.connect("app");
    let project = l.project(&app);
    let session = l.claude(&app, &project, "/bin/sh");
    let mut modc = l.connect("mod");
    assert!(matches!(
        l.request(
            &modc,
            Request::ModAttach {
                session_id: session.id.clone()
            }
        ),
        Response::Ok
    ));
    let hookc = l.connect("hook");
    l.hook(
        &hookc,
        &session.id,
        json!({"hook_event_name": "PermissionRequest", "tool_name": "Bash",
               "tool_input": {"command": "ls"}}),
    );
    if let Some(tool) = hold {
        l.mod_line(&modc, &session.id, json!({"t": "perm_hold", "tool": tool}));
    }
    modc.commands();
    let session = l.session(&session.id);
    assert_eq!(session.status, Status::Perm, "the prompt is open");
    AtPrompt {
        l,
        app,
        modc,
        session,
    }
}

#[test]
fn allow_inside_the_hold_window_is_answered_through_the_mod() {
    let mut t = at_prompt(Some("Bash"));
    let id = t.session.id.clone();
    // the shell shows no permission prompt, so only the mod path can say Ok
    let r = t.l.resolve(&t.app, &id, PermDecision::Allow);
    assert!(matches!(r, Response::Ok), "{r:?}");
    let prompt_id = t.session.perm.and_then(|p| p.id);
    assert_eq!(
        t.modc.commands(),
        [ModCommand::Answer {
            prompt_id,
            decision: PermDecision::Allow
        }]
    );
}

#[test]
fn deny_inside_the_hold_window_is_answered_through_the_mod() {
    let mut t = at_prompt(Some("Bash"));
    let id = t.session.id.clone();
    let r = t.l.resolve(&t.app, &id, PermDecision::Deny);
    assert!(matches!(r, Response::Ok), "{r:?}");
    assert!(matches!(
        t.modc.commands().as_slice(),
        [ModCommand::Answer {
            decision: PermDecision::Deny,
            ..
        }]
    ));
}

#[test]
fn a_decision_answers_the_held_hook_once() {
    let mut t = at_prompt(Some("Bash"));
    let id = t.session.id.clone();
    assert!(matches!(
        t.l.resolve(&t.app, &id, PermDecision::Allow),
        Response::Ok
    ));
    // the hold is spent: the second one takes the keystroke path (nothing to type on a shell)
    let again = t.l.resolve(&t.app, &id, PermDecision::Allow);
    assert!(matches!(again, Response::Error(_)), "{again:?}");
    assert_eq!(t.modc.commands().len(), 1);
}

#[test]
fn without_a_hold_the_decision_takes_the_keystroke_path() {
    let mut t = at_prompt(None);
    let id = t.session.id.clone();
    let r = t.l.resolve(&t.app, &id, PermDecision::Allow);
    let Response::Error(e) = r else {
        panic!("answered without a hold: {r:?}")
    };
    assert_eq!(
        e.code,
        ErrorCode::Conflict,
        "no prompt on the shell's screen"
    );
    assert!(t.modc.commands().is_empty());
}

#[test]
fn a_released_hold_is_gone() {
    let mut t = at_prompt(Some("Bash"));
    let id = t.session.id.clone();
    let modc = t.l.connect("mod");
    t.l.mod_line(&modc, &id, json!({"t": "perm_release"}));
    let r = t.l.resolve(&t.app, &id, PermDecision::Allow);
    assert!(matches!(r, Response::Error(_)), "{r:?}");
    assert!(t.modc.commands().is_empty());
}

#[test]
fn a_hold_for_another_tool_does_not_answer_this_prompt() {
    let mut t = at_prompt(Some("Edit"));
    let id = t.session.id.clone();
    let r = t.l.resolve(&t.app, &id, PermDecision::Allow);
    assert!(matches!(r, Response::Error(_)), "{r:?}");
    assert!(t.modc.commands().is_empty());
}

#[test]
fn always_is_never_sent_to_the_mod() {
    let mut t = at_prompt(Some("Bash"));
    let id = t.session.id.clone();
    let r = t.l.resolve(&t.app, &id, PermDecision::Always);
    assert!(matches!(r, Response::Error(_)), "{r:?}");
    assert!(t.modc.commands().is_empty());
}

#[test]
fn a_stale_prompt_id_is_refused_and_nothing_reaches_the_mod() {
    let mut t = at_prompt(Some("Bash"));
    let id = t.session.id.clone();
    let r = t.l.request(
        &t.app,
        Request::ResolvePermission {
            session_id: id,
            decision: PermDecision::Allow,
            via: None,
            prompt_id: Some("older-prompt".into()),
        },
    );
    let Response::Error(e) = r else {
        panic!("answered a stale prompt: {r:?}")
    };
    assert_eq!(e.code, ErrorCode::Conflict);
    assert!(t.modc.commands().is_empty());
}

#[test]
fn only_the_app_decides_never_the_mod_or_the_hook() {
    let mut t = at_prompt(Some("Bash"));
    let id = t.session.id.clone();
    for name in ["mod", "hook"] {
        let who = t.l.connect(name);
        let r = t.l.resolve(&who, &id, PermDecision::Allow);
        let Response::Error(e) = r else {
            panic!("{name} decided: {r:?}")
        };
        assert_eq!(e.code, ErrorCode::InvalidRequest, "{name}");
    }
    assert!(t.modc.commands().is_empty());
}

#[test]
fn a_hold_line_from_anyone_but_the_mod_is_refused() {
    let mut t = at_prompt(None);
    let id = t.session.id.clone();
    let hookc = t.l.connect("hook");
    let r = t.l.hook(
        &hookc,
        &id,
        json!({"kind": "lotse_mod", "t": "perm_hold", "tool": "Bash"}),
    );
    assert!(matches!(r, Response::Error(_)), "{r:?}");
    let r = t.l.resolve(&t.app, &id, PermDecision::Allow);
    assert!(matches!(r, Response::Error(_)), "{r:?}");
}

#[test]
fn a_composer_send_goes_to_the_attached_mod_and_is_not_typed() {
    let mut t = at_prompt(None);
    let id = t.session.id.clone();
    let r = t.l.request(
        &t.app,
        Request::SendInput {
            session_id: id,
            text: format!("{SUBMIT_MARKER}run the tests\nthen lint"),
        },
    );
    assert!(matches!(r, Response::Ok), "{r:?}");
    assert_eq!(
        t.modc.commands(),
        [ModCommand::Submit {
            text: "run the tests\nthen lint".into()
        }]
    );
}

#[test]
fn a_mod_may_not_send_a_composer_text() {
    let mut t = at_prompt(None);
    let id = t.session.id.clone();
    let r = t.l.request(
        &t.modc,
        Request::SendInput {
            session_id: id,
            text: format!("{SUBMIT_MARKER}rm -rf"),
        },
    );
    let Response::Error(e) = r else {
        panic!("the mod typed: {r:?}")
    };
    assert_eq!(e.code, ErrorCode::InvalidRequest);
    assert!(t.modc.commands().is_empty());
}

/// Everything `cat` echoed for this connection's attach so far.
fn echoed(conn: &mut Conn) -> String {
    let mut text = String::new();
    for frame in conn.frames() {
        if let Frame::Pty { data, .. } = frame {
            text.push_str(&String::from_utf8_lossy(&data));
        }
    }
    text
}

fn wait_echo(conn: &mut Conn, needle: &str) -> bool {
    let end = Instant::now() + Duration::from_secs(5);
    let mut seen = String::new();
    while Instant::now() < end {
        seen.push_str(&echoed(conn));
        if seen.contains(needle) {
            return true;
        }
        std::thread::sleep(Duration::from_millis(25));
    }
    false
}

fn typed_session() -> (Local, Conn, Conn, Session) {
    let mut l = Local::new();
    let mut app = l.connect("app");
    let project = l.project(&app);
    let session = l.claude(&app, &project, "/bin/cat");
    // an attach answers with a frame of its own (the replay follows), not a plain response
    l.next_req += 1;
    let attach = Request::Attach {
        session_id: session.id.clone(),
        cols: 80,
        rows: 24,
    };
    let answer =
        l.rt.block_on(l.state.handle(&app.client, l.next_req, attach));
    assert!(answer.is_none(), "{answer:?}");
    app.frames();
    let modc = l.connect("mod");
    (l, app, modc, session)
}

#[test]
fn a_composer_send_without_a_mod_is_typed_as_a_paste_with_its_return() {
    let (mut l, mut app, _modc, session) = typed_session();
    let r = l.request(
        &app,
        Request::SendInput {
            session_id: session.id.clone(),
            text: format!("{SUBMIT_MARKER}typed-by-fallback"),
        },
    );
    assert!(matches!(r, Response::Ok), "{r:?}");
    assert!(wait_echo(&mut app, "typed-by-fallback"));
}

#[test]
fn a_submit_the_mod_reports_failed_is_typed_instead() {
    let (mut l, mut app, mut modc, session) = typed_session();
    let attach = Request::ModAttach {
        session_id: session.id.clone(),
    };
    assert!(matches!(l.request(&modc, attach), Response::Ok));
    let r = l.request(
        &app,
        Request::SendInput {
            session_id: session.id.clone(),
            text: format!("{SUBMIT_MARKER}retry-by-keys"),
        },
    );
    assert!(matches!(r, Response::Ok), "{r:?}");
    assert_eq!(modc.commands().len(), 1);
    l.mod_line(
        &modc,
        &session.id,
        json!({"t": "submit_result", "ok": false}),
    );
    assert!(wait_echo(&mut app, "retry-by-keys"));
}

#[test]
fn a_submit_the_mod_reports_done_is_not_typed_too() {
    let (mut l, mut app, mut modc, session) = typed_session();
    let attach = Request::ModAttach {
        session_id: session.id.clone(),
    };
    assert!(matches!(l.request(&modc, attach), Response::Ok));
    l.request(
        &app,
        Request::SendInput {
            session_id: session.id.clone(),
            text: format!("{SUBMIT_MARKER}only-once"),
        },
    );
    modc.commands();
    l.mod_line(
        &modc,
        &session.id,
        json!({"t": "submit_result", "ok": true}),
    );
    assert!(!wait_echo(&mut app, "only-once"));
}
