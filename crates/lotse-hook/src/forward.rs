//! Forward one agent hook payload to the `lotsed` Unix socket.
//!
//! Contract with the agent (Claude Code runs this as a hook command): print **nothing** (hook stdout
//! can be fed back to the model, e.g. for `UserPromptSubmit`), exit 0 whatever happens, and finish
//! fast. Alpha never answers permission decisions, so no decision JSON is ever written either.
//!
//! Wire: `hello` (token from the token file, client `hook`) then `hook_event` request, one
//! connection per hook call, see `lotse-proto`. The whole exchange is bounded by `Config::deadline`.

use anyhow::{Context, Result, bail};
use lotse_proto::{
    AgentKind, Frame, Hello, Message, PROTO_VERSION, Request, RequestEnvelope, Response,
    ResponseEnvelope, SessionId, encode, read_frame,
};
use std::ffi::OsStr;
use std::io::{Read, Write};
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::time::Duration;

/// Largest hook payload read from stdin (tool output can be big). Beyond it the call is skipped.
pub const MAX_STDIN_BYTES: u64 = 8 * 1024 * 1024;

/// Total time for connect, send and the daemon's answer.
pub const DEFAULT_DEADLINE: Duration = Duration::from_millis(1500);

/// Env var the daemon sets on every PTY it spawns; hooks outside Lotse sessions do nothing.
pub const SESSION_ENV: &str = "LOTSE_SESSION_ID";

/// Socket path, exported by the daemon on every PTY it spawns.
pub const SOCKET_ENV: &str = "LOTSE_SOCKET";

/// Path of the token file (never the token itself), exported by the daemon next to [`SOCKET_ENV`].
pub const TOKEN_FILE_ENV: &str = "LOTSE_TOKEN_FILE";

/// Optional override of the Lotse data directory (the one holding `lotsed.sock` and `token`);
/// default `~/Library/Application Support/Lotse`. Only a fallback for [`SOCKET_ENV`] / [`TOKEN_FILE_ENV`].
pub const DATA_DIR_ENV: &str = "LOTSE_DATA_DIR";

#[derive(Debug, Clone)]
pub struct Config {
    pub socket: PathBuf,
    pub token_file: PathBuf,
    /// `None` or empty: not a Lotse session, nothing is sent.
    pub session_id: Option<String>,
    pub agent: AgentKind,
    pub deadline: Duration,
    /// The hook comes from the plugin Lotse injects into the pane (`--injected`), not from the
    /// user's global settings: the payload is tagged [`SOURCE_FIELD`] so the daemon can drop the
    /// twin event when both are installed.
    pub injected: bool,
}

/// Payload field the daemon reads (and removes) to tell injected hooks from global ones.
pub const SOURCE_FIELD: &str = "lotse_source";
/// Value of [`SOURCE_FIELD`] for an injected hook.
pub const SOURCE_INJECTED: &str = "injected";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outcome {
    /// The daemon accepted the event.
    Sent,
    /// `LOTSE_SESSION_ID` is not set: the agent runs outside Lotse.
    NoSession,
    /// stdin was not a JSON object, or too large.
    BadInput,
}

/// The Lotse data directory: `override_dir` (from [`DATA_DIR_ENV`]) when set and non-empty, else
/// `<platform data dir>/Lotse`.
pub fn data_dir(override_dir: Option<&OsStr>) -> Option<PathBuf> {
    match override_dir.filter(|d| !d.is_empty()) {
        Some(dir) => Some(PathBuf::from(dir)),
        None => dirs::data_dir().map(|d| d.join("Lotse")),
    }
}

impl Config {
    /// The daemon's contract: `LOTSE_SOCKET` + `LOTSE_TOKEN_FILE` (both set, else the data
    /// directory fallback) and `LOTSE_SESSION_ID`. The token is only ever read from its file.
    pub fn from_env(
        agent: AgentKind,
        get: impl Fn(&str) -> Option<std::ffi::OsString>,
    ) -> Option<Config> {
        let non_empty = |name: &str| get(name).filter(|v| !v.is_empty());
        let session_id = non_empty(SESSION_ENV).and_then(|s| s.into_string().ok());
        let mut cfg = match (non_empty(SOCKET_ENV), non_empty(TOKEN_FILE_ENV)) {
            (Some(socket), Some(token_file)) => Config {
                socket: PathBuf::from(socket),
                token_file: PathBuf::from(token_file),
                session_id: None,
                agent,
                deadline: DEFAULT_DEADLINE,
                injected: false,
            },
            _ => Config::in_data_dir(&data_dir(get(DATA_DIR_ENV).as_deref())?, agent, None),
        };
        cfg.session_id = session_id;
        Some(cfg)
    }

    /// Socket and token paths inside `data_dir`.
    pub fn in_data_dir(data_dir: &Path, agent: AgentKind, session_id: Option<String>) -> Config {
        Config {
            socket: data_dir.join("lotsed.sock"),
            token_file: data_dir.join("token"),
            session_id,
            agent,
            deadline: DEFAULT_DEADLINE,
            injected: false,
        }
    }
}

/// Read the hook payload from `stdin` and send it. Errors mean "not delivered"; callers ignore them.
pub fn run(cfg: &Config, stdin: impl Read) -> Result<Outcome> {
    // Always drain stdin, even outside Lotse, so the agent never sees a broken pipe.
    let mut raw = Vec::new();
    stdin
        .take(MAX_STDIN_BYTES + 1)
        .read_to_end(&mut raw)
        .context("reading stdin")?;
    let Some(session_id) = cfg.session_id.as_deref().filter(|s| !s.is_empty()) else {
        return Ok(Outcome::NoSession);
    };
    if raw.len() as u64 > MAX_STDIN_BYTES {
        return Ok(Outcome::BadInput);
    }
    let mut payload: serde_json::Value = match serde_json::from_slice(&raw) {
        Ok(v @ serde_json::Value::Object(_)) => v,
        _ => return Ok(Outcome::BadInput),
    };
    // Tool output can be huge and no adapter reads it: leave it out of the frame.
    if let Some(fields) = payload.as_object_mut() {
        fields.remove("tool_response");
        if cfg.agent == AgentKind::ClaudeCode {
            minimize_claude(fields);
        }
        if cfg.injected {
            fields.insert(SOURCE_FIELD.into(), SOURCE_INJECTED.into());
        }
    }
    let request = Request::HookEvent {
        agent: cfg.agent,
        session_id: Some(SessionId::new(session_id)),
        payload,
    };
    deliver(cfg, request)?;
    Ok(Outcome::Sent)
}

/// Fields of a Claude Code hook payload that carry conversation text; the adapter never reads them.
const CLAUDE_TEXT_FIELDS: [&str; 3] = ["prompt", "last_assistant_message", "message"];

/// The `tool_input` keys the adapter turns into a card summary (`summarize_input` in
/// `lotse-detect/src/claude.rs`). Everything else in a tool input (file contents, edits) stays behind.
const CLAUDE_TOOL_INPUT_KEYS: [&str; 8] = [
    "command",
    "file_path",
    "notebook_path",
    "path",
    "url",
    "pattern",
    "query",
    "description",
];

/// Data minimisation: the daemon gets what status detection reads, not the user's prompts, replies
/// or the contents of files the agent writes.
fn minimize_claude(fields: &mut serde_json::Map<String, serde_json::Value>) {
    for key in CLAUDE_TEXT_FIELDS {
        fields.remove(key);
    }
    if let Some(serde_json::Value::Object(input)) = fields.get_mut("tool_input") {
        input.retain(|k, _| CLAUDE_TOOL_INPUT_KEYS.contains(&k.as_str()));
    }
}

/// Run the exchange on a worker thread so even a connect that hangs cannot outlive the deadline.
fn deliver(cfg: &Config, request: Request) -> Result<()> {
    let (tx, rx) = mpsc::channel();
    let worker_cfg = cfg.clone();
    std::thread::spawn(move || {
        let _ = tx.send(exchange(&worker_cfg, request));
    });
    match rx.recv_timeout(cfg.deadline) {
        Ok(result) => result,
        Err(_) => bail!("timed out after {:?}", cfg.deadline),
    }
}

fn exchange(cfg: &Config, request: Request) -> Result<()> {
    let token = std::fs::read_to_string(&cfg.token_file)
        .with_context(|| format!("reading {}", cfg.token_file.display()))?
        .trim()
        .to_owned();
    let mut stream = UnixStream::connect(&cfg.socket)
        .with_context(|| format!("connecting to {}", cfg.socket.display()))?;
    // Per-operation timeouts, set once: macOS refuses `setsockopt` (EINVAL) on a socket whose peer
    // has already closed, so they must not be re-armed after the daemon answered. `deliver` bounds
    // the whole exchange regardless, so a failure to set them is not fatal.
    let _ = stream.set_write_timeout(Some(cfg.deadline));
    let _ = stream.set_read_timeout(Some(cfg.deadline));

    let hello = Frame::Control(Message::Hello(Hello {
        proto_version: PROTO_VERSION,
        token,
        client: "hook".into(),
    }));
    let request = Frame::Control(Message::Request(RequestEnvelope { req_id: 1, request }));
    let mut wire = encode(&hello)?;
    wire.extend_from_slice(&encode(&request)?);
    stream.write_all(&wire).context("sending the hook event")?;

    // Wait for the answers: a rejected hello comes back as `req_id: 0` + `Response::Error`.
    loop {
        let Some(frame) = read_frame(&mut stream).context("reading the daemon's answer")? else {
            bail!("daemon closed the connection without answering");
        };
        match frame {
            Frame::Control(Message::HelloOk(_)) => {}
            Frame::Control(Message::Response(ResponseEnvelope {
                req_id: 1,
                response,
            })) => {
                return match response {
                    Response::Error(e) => {
                        bail!("daemon refused the event: {:?}: {}", e.code, e.message)
                    }
                    _ => Ok(()),
                };
            }
            Frame::Control(Message::Response(ResponseEnvelope {
                response: Response::Error(e),
                ..
            })) => {
                bail!("daemon refused the connection: {:?}: {}", e.code, e.message)
            }
            // Pushed events and PTY frames are not for us.
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use lotse_proto::{ErrorCode, ProtoError};
    use std::os::unix::net::UnixListener;
    use std::sync::mpsc::Receiver;
    use std::thread::JoinHandle;
    use std::time::Instant;

    /// Fake daemon: accepts one connection, reads hello + request, answers as scripted.
    struct FakeDaemon {
        dir: tempfile::TempDir,
        received: Receiver<(Hello, RequestEnvelope)>,
        handle: JoinHandle<()>,
    }

    #[derive(Clone, Copy)]
    enum Mode {
        Accept,
        RejectHello,
        RejectEvent,
        Silent,
    }

    fn fake_daemon(mode: Mode, token: &str) -> FakeDaemon {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("token"), format!("{token}\n")).unwrap();
        let listener = UnixListener::bind(dir.path().join("lotsed.sock")).unwrap();
        let (tx, received) = mpsc::channel();
        let handle = std::thread::spawn(move || {
            let (mut conn, _) = listener.accept().unwrap();
            let Some(Frame::Control(Message::Hello(hello))) = read_frame(&mut conn).unwrap() else {
                panic!("expected hello first");
            };
            if matches!(mode, Mode::RejectHello) {
                let err = Message::Response(ResponseEnvelope {
                    req_id: 0,
                    response: Response::Error(ProtoError {
                        code: ErrorCode::Unauthorized,
                        message: "bad token".into(),
                    }),
                });
                lotse_proto::write_frame(&mut conn, &Frame::Control(err)).unwrap();
                return;
            }
            let Some(Frame::Control(Message::Request(req))) = read_frame(&mut conn).unwrap() else {
                panic!("expected a request after hello");
            };
            tx.send((hello, req.clone())).unwrap();
            let mut reply = |m| lotse_proto::write_frame(&mut conn, &Frame::Control(m)).unwrap();
            match mode {
                Mode::Silent => std::thread::sleep(Duration::from_secs(3)),
                Mode::RejectEvent => reply(Message::Response(ResponseEnvelope {
                    req_id: req.req_id,
                    response: Response::Error(ProtoError {
                        code: ErrorCode::NotFound,
                        message: "no such session".into(),
                    }),
                })),
                _ => {
                    reply(Message::HelloOk(lotse_proto::HelloOk {
                        proto_version: PROTO_VERSION,
                        daemon_version: "test".into(),
                    }));
                    reply(Message::Response(ResponseEnvelope {
                        req_id: req.req_id,
                        response: Response::Ok,
                    }));
                }
            }
        });
        FakeDaemon {
            dir,
            received,
            handle,
        }
    }

    fn cfg(d: &FakeDaemon, session: Option<&str>) -> Config {
        let mut cfg = Config::in_data_dir(
            d.dir.path(),
            AgentKind::ClaudeCode,
            session.map(str::to_owned),
        );
        cfg.deadline = Duration::from_millis(600);
        cfg
    }

    #[test]
    fn forwards_payload_with_session_id_and_token() {
        let d = fake_daemon(Mode::Accept, "s3cret");
        let payload = br#"{"hook_event_name":"PostToolUse","tool_name":"Bash","tool_input":{"command":"ls"},"tool_response":{"stdout":"big"}}"#;
        let out = run(&cfg(&d, Some("a1")), &payload[..]).unwrap();
        assert_eq!(out, Outcome::Sent);
        let (hello, req) = d.received.recv().unwrap();
        assert_eq!(hello.token, "s3cret");
        assert_eq!(hello.client, "hook");
        assert_eq!(hello.proto_version, PROTO_VERSION);
        assert_eq!(req.req_id, 1);
        assert_eq!(
            req.request,
            Request::HookEvent {
                agent: AgentKind::ClaudeCode,
                session_id: Some(SessionId::new("a1")),
                // Everything verbatim except the tool output.
                payload: serde_json::json!({
                    "hook_event_name": "PostToolUse",
                    "tool_name": "Bash",
                    "tool_input": {"command": "ls"},
                }),
            }
        );
        d.handle.join().unwrap();
    }

    #[test]
    fn claude_payloads_leave_conversation_text_and_file_contents_behind() {
        let d = fake_daemon(Mode::Accept, "t");
        let payload = br#"{"hook_event_name":"UserPromptSubmit","prompt":"my secret plan","last_assistant_message":"reply text","message":"note","transcript_path":"/t.jsonl","tool_name":"Write","tool_use_id":"tu1","tool_input":{"file_path":"/a/b.rs","content":"fn secret() {}","old_string":"x","new_string":"y"}}"#;
        run(&cfg(&d, Some("a1")), &payload[..]).unwrap();
        let (_, req) = d.received.recv().unwrap();
        let Request::HookEvent { payload, .. } = req.request else {
            panic!("expected a hook event");
        };
        // what the adapter reads stays: event, tool, ids and the summary key of the input
        assert_eq!(
            payload,
            serde_json::json!({
                "hook_event_name": "UserPromptSubmit",
                "transcript_path": "/t.jsonl",
                "tool_name": "Write",
                "tool_use_id": "tu1",
                "tool_input": {"file_path": "/a/b.rs"},
            })
        );
        d.handle.join().unwrap();
    }

    #[test]
    fn an_injected_hook_tags_the_payload_so_the_daemon_can_drop_a_twin() {
        let d = fake_daemon(Mode::Accept, "t");
        let mut cfg = cfg(&d, Some("a1"));
        cfg.injected = true;
        run(&cfg, &br#"{"hook_event_name":"Stop"}"#[..]).unwrap();
        let (_, req) = d.received.recv().unwrap();
        let Request::HookEvent { payload, .. } = req.request else {
            panic!("expected a hook event");
        };
        assert_eq!(
            payload,
            serde_json::json!({"hook_event_name": "Stop", "lotse_source": "injected"})
        );
        d.handle.join().unwrap();
    }

    #[test]
    fn no_session_id_sends_nothing_but_drains_stdin() {
        let d = fake_daemon(Mode::Accept, "t");
        let mut input = std::io::Cursor::new(br#"{"hook_event_name":"Stop"}"#.to_vec());
        assert_eq!(run(&cfg(&d, None), &mut input).unwrap(), Outcome::NoSession);
        assert_eq!(
            input.position(),
            input.get_ref().len() as u64,
            "stdin fully read"
        );
        assert_eq!(
            run(&cfg(&d, Some("")), &b"{}"[..]).unwrap(),
            Outcome::NoSession
        );
        assert!(d.received.try_recv().is_err(), "nothing reached the daemon");
        // Unblock the fake daemon's accept so the thread ends.
        let _ = UnixStream::connect(d.dir.path().join("lotsed.sock"));
    }

    #[test]
    fn bad_input_is_skipped() {
        let d = fake_daemon(Mode::Accept, "t");
        for bad in [&b""[..], b"not json", b"[1,2]", b"\"str\"", b"{\"a\":"] {
            assert_eq!(run(&cfg(&d, Some("a1")), bad).unwrap(), Outcome::BadInput);
        }
        let big = vec![b' '; MAX_STDIN_BYTES as usize + 1];
        assert_eq!(
            run(&cfg(&d, Some("a1")), &big[..]).unwrap(),
            Outcome::BadInput
        );
        assert!(d.received.try_recv().is_err());
        let _ = UnixStream::connect(d.dir.path().join("lotsed.sock"));
    }

    #[test]
    fn missing_socket_or_token_is_an_error_not_a_hang() {
        let dir = tempfile::tempdir().unwrap();
        let cfg = Config::in_data_dir(dir.path(), AgentKind::ClaudeCode, Some("a1".into()));
        let started = Instant::now();
        assert!(run(&cfg, &b"{}"[..]).is_err(), "no token file");
        std::fs::write(dir.path().join("token"), "t").unwrap();
        assert!(run(&cfg, &b"{}"[..]).is_err(), "no socket");
        assert!(started.elapsed() < Duration::from_secs(5));
    }

    #[test]
    fn rejected_hello_and_rejected_event_are_errors() {
        let d = fake_daemon(Mode::RejectHello, "wrong");
        let err = run(&cfg(&d, Some("a1")), &b"{}"[..])
            .unwrap_err()
            .to_string();
        assert!(err.contains("refused the connection"), "{err}");
        d.handle.join().unwrap();

        let d = fake_daemon(Mode::RejectEvent, "t");
        let err = run(&cfg(&d, Some("zz")), &b"{}"[..])
            .unwrap_err()
            .to_string();
        assert!(err.contains("refused the event"), "{err}");
    }

    #[test]
    fn a_daemon_that_never_answers_cannot_block_past_the_deadline() {
        let d = fake_daemon(Mode::Silent, "t");
        let mut cfg = cfg(&d, Some("a1"));
        cfg.deadline = Duration::from_millis(300);
        let started = Instant::now();
        assert!(run(&cfg, &b"{}"[..]).is_err());
        assert!(
            started.elapsed() < Duration::from_millis(2500),
            "{:?}",
            started.elapsed()
        );
    }

    #[test]
    fn data_dir_prefers_the_override() {
        assert_eq!(
            data_dir(Some(OsStr::new("/tmp/x"))),
            Some(PathBuf::from("/tmp/x"))
        );
        // Empty override falls back to the platform directory (never read in tests beyond its shape).
        if let Some(dir) = data_dir(Some(OsStr::new(""))) {
            assert!(dir.ends_with("Lotse"));
        }
    }
}
