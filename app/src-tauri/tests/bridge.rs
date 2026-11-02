//! The Tauri bridge against the real `lotsed` binary, without a window: the app starts the daemon,
//! requests/events/PTY flow, status events reach the sink, "quit and relaunch" finds the session
//! with its scrollback, and a dead daemon is restarted and reconnected.
//! Temp data dir + `--no-login-env`: nothing of the real Lotse data, `~/.claude` or dotfiles is touched.

use lotse_app_lib::daemon::{Bridge, Config, Sink};
use lotse_proto::{
    AgentKind, Event, NewProject, NewSession, NotificationSettings, PROTO_VERSION, Request,
    Response, Session, SessionId, Status,
};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

#[derive(Default)]
struct Recorder {
    events: Mutex<Vec<Event>>,
    pty: Mutex<Vec<u8>>,
    states: Mutex<Vec<&'static str>>,
    settings: Mutex<Vec<NotificationSettings>>,
    /// Runs once, right after the first notification toggles arrive (a reconnect is then still in progress).
    on_settings: Mutex<Option<Box<dyn FnOnce() + Send>>>,
}

impl Sink for Recorder {
    fn event(&self, event: &Event) {
        self.events.lock().unwrap().push(event.clone());
    }
    fn connection(&self, state: &'static str) {
        self.states.lock().unwrap().push(state);
    }
    fn pty(&self, _session_id: &SessionId, data: &[u8]) {
        self.pty.lock().unwrap().extend_from_slice(data);
    }
    fn notification_settings(&self, settings: NotificationSettings) {
        self.settings.lock().unwrap().push(settings);
        let hook = self.on_settings.lock().unwrap().take();
        if let Some(hook) = hook {
            hook();
        }
    }
}

fn wait_for(what: &str, mut ok: impl FnMut() -> bool) {
    let end = Instant::now() + Duration::from_secs(20);
    while Instant::now() < end {
        if ok() {
            return;
        }
        std::thread::sleep(Duration::from_millis(25));
    }
    panic!("timed out waiting for {what}");
}

fn binary(name: &str) -> PathBuf {
    let exe = std::env::current_exe().unwrap();
    let p = exe.parent().unwrap().parent().unwrap().join(name);
    assert!(
        p.is_file(),
        "{name} is not built next to the tests: run `cargo build --workspace` (cargo test --workspace builds it)"
    );
    p
}

struct Setup {
    // keeps the temp dir alive
    tmp: tempfile::TempDir,
    config: Config,
    rt: tokio::runtime::Runtime,
}

fn setup() -> Setup {
    let tmp = tempfile::Builder::new().prefix("lb").tempdir().unwrap();
    let data = tmp.path().join("d");
    let config = Config {
        data_dir: Some(data),
        daemon_bin: Some(binary("lotsed")),
        daemon_args: vec!["--no-login-env".into()],
    };
    Setup {
        tmp,
        config,
        rt: tokio::runtime::Runtime::new().unwrap(),
    }
}

fn open(s: &Setup) -> (Arc<Bridge>, Arc<Recorder>) {
    let bridge = Arc::new(Bridge::with_config(s.config.clone()));
    let rec = Arc::new(Recorder::default());
    bridge.set_sink(rec.clone());
    (bridge, rec)
}

fn call(s: &Setup, b: &Bridge, r: Request) -> Response {
    s.rt.block_on(b.request(r)).expect("daemon answers")
}

fn session(r: Response) -> Session {
    match r {
        Response::Session(s) => s,
        other => panic!("expected a session, got {other:?}"),
    }
}

fn new_session(project: &lotse_proto::Project, agent: AgentKind) -> Request {
    Request::CreateSession(NewSession {
        project_id: project.id.clone(),
        agent,
        command: Some("/bin/sh".into()),
        title: None,
        worktree: None,
        first_prompt: None,
    })
}

fn attach(s: &Setup, b: &Bridge, id: &SessionId) {
    match call(
        s,
        b,
        Request::Attach {
            session_id: id.clone(),
            cols: 100,
            rows: 30,
        },
    ) {
        Response::Attached(_) => {}
        other => panic!("attach: {other:?}"),
    }
}

fn pty_text(rec: &Recorder) -> String {
    String::from_utf8_lossy(&rec.pty.lock().unwrap()).into_owned()
}

fn shutdown_daemon(s: &Setup, b: &Bridge) {
    let _ = s.rt.block_on(b.request(Request::Shutdown));
    b.close();
}

/// The plan's done-when, at the bridge level: start the daemon, add a temp project, open a shell
/// session, status events flow to the sink, quit and relaunch, the session is still there with its
/// scrollback.
#[test]
fn app_starts_the_daemon_events_flow_and_a_relaunch_finds_the_session_with_scrollback() {
    let s = setup();
    let (a, rec) = open(&s);
    // 1. no daemon yet: connecting starts it
    assert!(
        !s.config
            .data_dir
            .as_ref()
            .unwrap()
            .join("lotsed.sock")
            .exists()
    );
    let hello = a
        .connect_blocking()
        .expect("the app starts lotsed and shakes hands");
    assert_eq!(hello.proto_version, PROTO_VERSION);
    assert!(a.is_connected());

    // 2. a temp project and a shell session
    let dir = s.tmp.path().join("temp-project");
    std::fs::create_dir_all(&dir).unwrap();
    let Response::Project(project) = call(
        &s,
        &a,
        Request::AddProject(NewProject {
            path: dir.to_string_lossy().into_owned(),
            name: None,
            color: None,
            icon: None,
        }),
    ) else {
        panic!("add_project")
    };
    let shell = session(call(&s, &a, new_session(&project, AgentKind::Terminal)));
    attach(&s, &a, &shell.id);
    a.write_pty(&shell.id, b"echo scrollback-$((6*7))\n")
        .unwrap();
    wait_for("shell output through the PTY path", || {
        pty_text(&rec).contains("scrollback-42\r")
    });

    // 3. status events: a hook (the real binary, with the env the daemon exports into the pane)
    //    moves a Claude session to Run, and the event reaches the sink
    let claude = session(call(&s, &a, new_session(&project, AgentKind::ClaudeCode)));
    attach(&s, &a, &claude.id);
    let hook = binary("lotse-hook");
    let cmd = format!(
        "printf '%s' '{{\"hook_event_name\":\"UserPromptSubmit\"}}' | '{}' claude\n",
        hook.display()
    );
    a.write_pty(&claude.id, cmd.as_bytes()).unwrap();
    wait_for("status_changed → run", || {
        rec.events.lock().unwrap().iter().any(|e| {
            matches!(e, Event::StatusChanged { session_id, status: Status::Run, .. } if *session_id == claude.id)
        })
    });

    // 4. quit the app (daemon and sessions keep running) and relaunch
    a.close();
    let (b, rec2) = open(&s);
    b.connect_blocking()
        .expect("relaunch connects to the running daemon");
    let Response::Snapshot(snap) = call(&s, &b, Request::GetSnapshot) else {
        panic!("snapshot")
    };
    assert_eq!(snap.projects.len(), 1);
    let ids: Vec<_> = snap.sessions.iter().map(|x| x.id.clone()).collect();
    assert!(
        ids.contains(&shell.id) && ids.contains(&claude.id),
        "sessions survived: {ids:?}"
    );
    let again = snap.sessions.iter().find(|x| x.id == claude.id).unwrap();
    assert_eq!(again.status, Status::Run, "the status survived too");
    attach(&s, &b, &shell.id);
    wait_for("scrollback replay", || {
        pty_text(&rec2).contains("scrollback-42")
    });
    shutdown_daemon(&s, &b);
}

#[test]
fn a_dead_daemon_is_noticed_restarted_and_reconnected() {
    let s = setup();
    let (a, rec) = open(&s);
    a.connect_blocking().unwrap();
    // the daemon exits (crash, `lotsed` killed, shutdown request)
    let _ = s.rt.block_on(a.request(Request::Shutdown));
    wait_for("disconnected", || {
        rec.states.lock().unwrap().contains(&"disconnected")
    });
    assert!(!a.is_connected());
    // requests fail fast while it is down
    assert!(s.rt.block_on(a.request(Request::Ping)).is_err());
    // the bridge starts a new daemon and reconnects by itself
    wait_for("reconnected", || {
        rec.states.lock().unwrap().contains(&"reconnected")
    });
    assert!(matches!(call(&s, &a, Request::Ping), Response::Pong));
    // the notifier got the fresh toggles (a persisted pause must not be ignored after a reconnect)
    assert!(!rec.settings.lock().unwrap().is_empty());
    shutdown_daemon(&s, &a);
}

/// The new link drops while the reconnect thread is still syncing settings: no stale "reconnected"
/// after "disconnected", and a fresh reconnect still starts.
#[test]
fn a_link_that_drops_during_the_reconnect_sync_is_not_announced_as_reconnected() {
    let s = setup();
    let (a, rec) = open(&s);
    a.connect_blocking().unwrap();
    let bridge = a.clone();
    *rec.on_settings.lock().unwrap() = Some(Box::new(move || {
        // kill the daemon (and so the just-installed link) before the reconnect thread announces it
        let rt = tokio::runtime::Runtime::new().unwrap();
        let _ = rt.block_on(bridge.request(Request::Shutdown));
        wait_for("the new link to drop", || !bridge.is_connected());
    }));
    let _ = s.rt.block_on(a.request(Request::Shutdown));
    wait_for("the second reconnect to finish", || {
        let states = rec.states.lock().unwrap();
        a.is_connected() && states.len() >= 3 && states.last() == Some(&"reconnected")
    });
    // let a stale announcement, if one is coming, arrive
    wait_for("the last settings sync", || {
        rec.settings.lock().unwrap().len() >= 2
    });
    assert_eq!(
        *rec.states.lock().unwrap(),
        ["disconnected", "disconnected", "reconnected"]
    );
    assert!(a.is_connected());
    shutdown_daemon(&s, &a);
}

#[test]
fn a_bad_token_or_missing_binary_is_reported_not_hung() {
    let s = setup();
    let missing = Config {
        daemon_bin: Some(s.tmp.path().join("no-such-lotsed")),
        ..s.config.clone()
    };
    let bridge = Arc::new(Bridge::with_config(missing));
    let err = bridge.connect_blocking().unwrap_err();
    assert!(err.contains("starting"), "{err}");
    assert!(!bridge.is_connected());
}
