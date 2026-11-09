//! Test harness: starts the real `lotsed` binary on a temp data dir and speaks the wire protocol
//! over the socket with a blocking client. Never touches `~/Library/Application Support/Lotse`,
//! `~/.claude` or the user's dotfiles (`--no-login-env`, temp HOME, fixed PATH).
#![allow(dead_code)]

use lotse_proto::{
    AgentKind, Event, Frame, Hello, Message, NewProject, NewSession, PROTO_VERSION, Project,
    Request, RequestEnvelope, Response, Session, SessionId, read_frame, write_frame,
};
use std::collections::HashMap;
use std::os::unix::fs::PermissionsExt;
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError};
use std::time::{Duration, Instant};
use tempfile::TempDir;

pub const WAIT: Duration = Duration::from_secs(10);

pub struct Daemon {
    pub child: Child,
    /// Short, so the socket path stays under the 104-byte `sun_path` limit.
    pub tmp: TempDir,
    pub data_dir: PathBuf,
    pub home: PathBuf,
    path_env: String,
    /// `SHELL` of the daemon; `Some` also switches the login-shell environment capture on.
    login_shell: Option<PathBuf>,
    /// Extra environment of the daemon (a parent agent's markers, user config).
    extra_env: Vec<(String, String)>,
}

impl Daemon {
    pub fn start() -> Self {
        Self::start_with_path(None)
    }

    /// `extra_path`: a directory put in front of PATH (fake `claude` binaries).
    pub fn start_with_path(extra_path: Option<&Path>) -> Self {
        Self::start_full(extra_path, None)
    }

    /// The daemon starts with `env` on top of its minimal environment, like a daemon launched from
    /// inside an agent session.
    pub fn start_with_env(env: &[(&str, &str)]) -> Self {
        let env = env
            .iter()
            .map(|(k, v)| ((*k).into(), (*v).into()))
            .collect();
        Self::start_inner(None, None, env)
    }

    /// Capture the environment from `shell -l -i -c …` like production does (use a fake shell script).
    pub fn start_with_login_shell(shell: &Path) -> Self {
        Self::start_full(None, Some(shell.to_owned()))
    }

    fn start_full(extra_path: Option<&Path>, login_shell: Option<PathBuf>) -> Self {
        Self::start_inner(extra_path, login_shell, Vec::new())
    }

    fn start_inner(
        extra_path: Option<&Path>,
        login_shell: Option<PathBuf>,
        extra_env: Vec<(String, String)>,
    ) -> Self {
        let tmp = tempfile::Builder::new().prefix("lt").tempdir().unwrap();
        let data_dir = tmp.path().join("data");
        let home = tmp.path().join("home");
        std::fs::create_dir_all(&home).unwrap();
        let path_env = match extra_path {
            Some(p) => format!("{}:/usr/bin:/bin", p.display()),
            None => "/usr/bin:/bin".to_owned(),
        };
        let child = spawn_child(
            &data_dir,
            &home,
            &path_env,
            login_shell.as_deref(),
            &extra_env,
        );
        let d = Self {
            child,
            tmp,
            data_dir,
            home,
            path_env,
            login_shell,
            extra_env,
        };
        d.wait_ready();
        d
    }

    /// Start again on the same data dir (after the previous daemon exited).
    pub fn restart(mut self) -> Self {
        let _ = self.child.kill();
        let _ = self.child.wait();
        self.child = spawn_child(
            &self.data_dir,
            &self.home,
            &self.path_env,
            self.login_shell.as_deref(),
            &self.extra_env,
        );
        self.wait_ready();
        self
    }

    fn wait_ready(&self) {
        let sock = self.socket();
        let start = Instant::now();
        while start.elapsed() < WAIT {
            if UnixStream::connect(&sock).is_ok() {
                return;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        panic!("lotsed did not come up");
    }

    pub fn pid(&self) -> u32 {
        self.child.id()
    }

    pub fn socket(&self) -> PathBuf {
        self.data_dir.join("lotsed.sock")
    }

    pub fn token_path(&self) -> PathBuf {
        self.data_dir.join("token")
    }

    pub fn token(&self) -> String {
        std::fs::read_to_string(self.token_path())
            .unwrap()
            .trim()
            .to_owned()
    }

    pub fn client(&self) -> Client {
        Client::connect(&self.socket(), &self.token())
    }

    /// A project folder inside the temp dir.
    pub fn project_dir(&self, name: &str) -> PathBuf {
        let p = self.tmp.path().join(name);
        std::fs::create_dir_all(&p).unwrap();
        p
    }

    /// Wait for the process to exit; `None` on timeout.
    pub fn wait_exit(&mut self, max: Duration) -> Option<std::process::ExitStatus> {
        let start = Instant::now();
        while start.elapsed() < max {
            if let Some(st) = self.child.try_wait().unwrap() {
                return Some(st);
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        None
    }
}

fn spawn_child(
    data_dir: &Path,
    home: &Path,
    path_env: &str,
    login_shell: Option<&Path>,
    extra_env: &[(String, String)],
) -> Child {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_lotsed"));
    cmd.arg("--data-dir").arg(data_dir);
    if login_shell.is_none() {
        cmd.arg("--no-login-env");
    }
    cmd.env_clear()
        .env("PATH", path_env)
        .env("HOME", home)
        .env("SHELL", login_shell.unwrap_or(Path::new("/bin/sh")))
        .env("RUST_LOG", "warn")
        .envs(extra_env.iter().map(|(k, v)| (k, v)))
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("start lotsed")
}

impl Drop for Daemon {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

pub struct Client {
    stream: UnixStream,
    rx: Receiver<Option<Frame>>,
    next_id: u64,
    pub events: Vec<Event>,
    pub output: HashMap<SessionId, Vec<u8>>,
}

impl Client {
    pub fn connect(socket: &Path, token: &str) -> Self {
        let mut stream = UnixStream::connect(socket).unwrap();
        hello(&mut stream, token, PROTO_VERSION);
        match read_frame(&mut stream).unwrap() {
            Some(Frame::Control(Message::HelloOk(ok))) => {
                assert_eq!(ok.proto_version, PROTO_VERSION)
            }
            other => panic!("expected hello_ok, got {other:?}"),
        }
        let (tx, rx) = mpsc::channel();
        let mut reader = stream.try_clone().unwrap();
        std::thread::spawn(move || {
            loop {
                match read_frame(&mut reader) {
                    Ok(Some(f)) => {
                        if tx.send(Some(f)).is_err() {
                            return;
                        }
                    }
                    _ => {
                        let _ = tx.send(None);
                        return;
                    }
                }
            }
        });
        Self {
            stream,
            rx,
            next_id: 0,
            events: Vec::new(),
            output: HashMap::new(),
        }
    }

    fn stash(&mut self, frame: Frame) -> Option<(u64, Response)> {
        match frame {
            Frame::Control(Message::Response(r)) => Some((r.req_id, r.response)),
            Frame::Control(Message::Event(e)) => {
                self.events.push(e);
                None
            }
            Frame::Pty { session_id, data } => {
                self.output.entry(session_id).or_default().extend(data);
                None
            }
            other => panic!("unexpected frame {other:?}"),
        }
    }

    pub fn send(&mut self, request: Request) -> u64 {
        self.next_id += 1;
        let req_id = self.next_id;
        write_frame(
            &mut self.stream,
            &Frame::Control(Message::Request(RequestEnvelope { req_id, request })),
        )
        .unwrap();
        req_id
    }

    pub fn request(&mut self, request: Request) -> Response {
        let id = self.send(request);
        self.response(id)
    }

    pub fn response(&mut self, id: u64) -> Response {
        let start = Instant::now();
        loop {
            let left = WAIT.saturating_sub(start.elapsed());
            match self.rx.recv_timeout(left) {
                Ok(Some(f)) => {
                    if let Some((rid, resp)) = self.stash(f) {
                        assert_eq!(rid, id, "response for another request");
                        return resp;
                    }
                }
                Ok(None) => panic!("connection closed while waiting for response {id}"),
                Err(_) => panic!("timeout waiting for response {id}"),
            }
        }
    }

    pub fn write_raw(&mut self, bytes: &[u8]) {
        use std::io::Write;
        self.stream.write_all(bytes).unwrap();
    }

    pub fn send_pty(&mut self, session: &SessionId, data: &[u8]) {
        write_frame(
            &mut self.stream,
            &Frame::Pty {
                session_id: session.clone(),
                data: data.to_vec(),
            },
        )
        .unwrap();
    }

    /// Pump frames for `max`; `true` when the closure sees what it wants.
    pub fn pump_until(&mut self, max: Duration, mut done: impl FnMut(&Client) -> bool) -> bool {
        let start = Instant::now();
        loop {
            if done(self) {
                return true;
            }
            let left = max.saturating_sub(start.elapsed());
            if left.is_zero() {
                return false;
            }
            match self.rx.recv_timeout(left) {
                Ok(Some(f)) => {
                    assert!(self.stash(f).is_none(), "unexpected response");
                }
                Ok(None) | Err(RecvTimeoutError::Disconnected) => return done(self),
                Err(RecvTimeoutError::Timeout) => return done(self),
            }
        }
    }

    pub fn wait_output(&mut self, session: &SessionId, needle: &str) -> bool {
        let s = session.clone();
        let mut scanned = 0;
        self.pump_until(WAIT, move |c| {
            c.output.get(&s).is_some_and(|o| {
                // linear in total: scan only what arrived since the last look (plus the overlap)
                let from = scanned.min(o.len()).saturating_sub(needle.len());
                scanned = o.len();
                contains(&o[from..], needle.as_bytes())
            })
        })
    }

    /// The number after `key` once the terminal printed it completely (digits, then more output).
    /// The tty echo of a typed `key=$!` has no digits and never matches.
    pub fn wait_number(&mut self, session: &SessionId, key: &str) -> i32 {
        let (s, k) = (session.clone(), key.to_owned());
        let found = self.pump_until(WAIT, move |c| {
            complete_number(&c.output_text(&s), &k).is_some()
        });
        assert!(
            found,
            "no `{key}<digits>` in {:?}",
            self.output_text(session)
        );
        complete_number(&self.output_text(session), key).unwrap()
    }

    /// Like `response`, but a PTY frame of `session` before the response is a protocol violation.
    fn response_before_stream(&mut self, id: u64, session: &SessionId) -> Response {
        let start = Instant::now();
        loop {
            let left = WAIT.saturating_sub(start.elapsed());
            match self.rx.recv_timeout(left) {
                Ok(Some(Frame::Pty { session_id, .. })) if session_id == *session => {
                    panic!("PTY frame of {session} arrived before the attached response")
                }
                Ok(Some(f)) => {
                    if let Some((rid, resp)) = self.stash(f) {
                        assert_eq!(rid, id);
                        return resp;
                    }
                }
                Ok(None) => panic!("connection closed while waiting for response {id}"),
                Err(_) => panic!("timeout waiting for response {id}"),
            }
        }
    }

    pub fn output_text(&self, session: &SessionId) -> String {
        String::from_utf8_lossy(self.output.get(session).map_or(&[][..], |v| v)).into_owned()
    }

    pub fn clear_output(&mut self) {
        self.output.clear();
    }

    /// Collect frames for `dur` (to assert that nothing arrives).
    pub fn drain(&mut self, dur: Duration) {
        self.pump_until(dur, |_| false);
    }

    /// First event the predicate accepts; searches stashed events first.
    pub fn wait_event(&mut self, pred: impl Fn(&Event) -> bool) -> Event {
        let mut found = None;
        let ok = self.pump_until(WAIT, |c| c.events.iter().any(&pred));
        if ok {
            let i = self.events.iter().position(&pred).unwrap();
            found = Some(self.events.remove(i));
        }
        found.expect("timeout waiting for event")
    }

    /// `true` when the daemon closed the connection.
    pub fn wait_closed(&mut self) -> bool {
        let start = Instant::now();
        while start.elapsed() < WAIT {
            match self.rx.recv_timeout(Duration::from_millis(100)) {
                Ok(None) | Err(RecvTimeoutError::Disconnected) => return true,
                Ok(Some(f)) => {
                    self.stash(f);
                }
                Err(RecvTimeoutError::Timeout) => {}
            }
        }
        false
    }

    // conveniences

    pub fn add_project(&mut self, dir: &Path) -> Project {
        match self.request(Request::AddProject(NewProject {
            path: dir.to_string_lossy().into_owned(),
            name: None,
            color: None,
            icon: None,
        })) {
            Response::Project(p) => p,
            other => panic!("add_project: {other:?}"),
        }
    }

    pub fn create_session(&mut self, project: &Project, command: Option<&str>) -> Session {
        match self.request(Request::CreateSession(NewSession {
            project_id: project.id.clone(),
            agent: AgentKind::Terminal,
            command: command.map(str::to_owned),
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

    /// Attach and wait until the whole replay arrived; returns the replay length.
    pub fn attach(&mut self, session: &SessionId, cols: u16, rows: u16) -> u64 {
        self.output.remove(session);
        let id = self.send(Request::Attach {
            session_id: session.clone(),
            cols,
            rows,
        });
        match self.response_before_stream(id, session) {
            Response::Attached(info) => {
                assert_eq!(info.session_id, *session);
                let s = session.clone();
                let want = info.replay_len as usize;
                assert!(
                    self.pump_until(WAIT, move |c| c.output.get(&s).map_or(0, Vec::len) >= want),
                    "replay of {want} bytes did not arrive"
                );
                info.replay_len
            }
            other => panic!("attach: {other:?}"),
        }
    }

    pub fn expect_ok(&mut self, request: Request) {
        match self.request(request) {
            Response::Ok => {}
            other => panic!("expected ok, got {other:?}"),
        }
    }
}

pub fn hello(stream: &mut UnixStream, token: &str, proto_version: u32) {
    write_frame(
        stream,
        &Frame::Control(Message::Hello(Hello {
            proto_version,
            token: token.to_owned(),
            client: "test".into(),
        })),
    )
    .unwrap();
}

/// `key` followed by digits and at least one more character (newest occurrence first).
pub fn complete_number(text: &str, key: &str) -> Option<i32> {
    let starts: Vec<usize> = text.match_indices(key).map(|(i, _)| i).collect();
    starts.into_iter().rev().find_map(|i| {
        let rest = &text[i + key.len()..];
        let digits: String = rest.chars().take_while(char::is_ascii_digit).collect();
        (!digits.is_empty() && rest.len() > digits.len())
            .then(|| digits.parse().ok())
            .flatten()
    })
}

pub fn contains(hay: &[u8], needle: &[u8]) -> bool {
    !needle.is_empty() && hay.windows(needle.len()).any(|w| w == needle)
}

/// Is a process with this pid alive (not a zombie we could still signal)?
pub fn pid_alive(pid: i32) -> bool {
    nix::sys::signal::kill(nix::unistd::Pid::from_raw(pid), None).is_ok()
}

pub fn wait_dead(pid: i32) -> bool {
    let start = Instant::now();
    while start.elapsed() < Duration::from_secs(5) {
        if !pid_alive(pid) {
            return true;
        }
        std::thread::sleep(Duration::from_millis(25));
    }
    false
}

/// Number after `key=` in terminal output (`pid=1234`).
pub fn number_after(text: &str, key: &str) -> Option<i32> {
    let rest = &text[text.rfind(key)? + key.len()..];
    let digits: String = rest.chars().take_while(char::is_ascii_digit).collect();
    digits.parse().ok()
}

/// Write an executable shell script.
pub fn script(dir: &Path, name: &str, body: &str) {
    let p = dir.join(name);
    std::fs::write(&p, format!("#!/bin/sh\n{body}\n")).unwrap();
    std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o755)).unwrap();
}
