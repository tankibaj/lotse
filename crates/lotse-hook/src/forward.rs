//! Forward one agent hook payload to the `lotsed` Unix socket.
//!
//! Contract with the agent (Claude Code runs this as a hook command): print **nothing** (hook stdout
//! can be fed back to the model, e.g. for `UserPromptSubmit`), exit 0 whatever happens, and finish
//! fast. The one exception is auto-allow (off until the user sets rules): for a `PreToolUse` call of
//! a pane's injected hook the daemon is asked whether a rule lets it skip its prompt, and only its
//! `ok` makes the hook print [`ALLOW_OUTPUT`]. No other decision JSON is ever written.
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
    run_answer(cfg, stdin).map(|(outcome, _)| outcome)
}

/// What a PreToolUse hook prints to let the call skip its permission prompt (an auto-allow rule).
pub const ALLOW_OUTPUT: &str = r#"{"hookSpecificOutput":{"hookEventName":"PreToolUse","permissionDecision":"allow","permissionDecisionReason":"Lotse auto-allow rule"}}"#;

/// `payload.kind` of the question "may this call skip its prompt?" (see `lotsed::autoallow`).
pub const AUTOALLOW_KIND: &str = "lotse_autoallow";

/// The tool input a rule can match on; everything else (file contents, edits) stays behind.
const AUTOALLOW_INPUT_KEYS: [&str; 5] = [
    "command",
    "file_path",
    "notebook_path",
    "path",
    "dangerouslyDisableSandbox",
];

/// The auto-allow question for a Claude `PreToolUse` payload, or `None` when it must not be asked:
/// another event, or a permission mode other than `default` (`plan` must never be talked out of its
/// restrictions, and the other modes do not prompt anyway).
pub fn autoallow_query(
    raw: &serde_json::Map<String, serde_json::Value>,
) -> Option<serde_json::Value> {
    use serde_json::Value;
    let named = |key: &str, want: &str| raw.get(key).and_then(Value::as_str) == Some(want);
    if !named("hook_event_name", "PreToolUse") || !named("permission_mode", "default") {
        return None;
    }
    let tool = raw.get("tool_name")?.as_str()?;
    let input = raw.get("tool_input")?.as_object()?;
    Some(autoallow_question(tool, input))
}

/// The question for `tool` and the keys of `input` a rule can match on.
pub fn autoallow_question(
    tool: &str,
    input: &serde_json::Map<String, serde_json::Value>,
) -> serde_json::Value {
    let kept: serde_json::Map<String, serde_json::Value> = AUTOALLOW_INPUT_KEYS
        .iter()
        .filter_map(|k| Some((k.to_string(), input.get(*k)?.clone())))
        .collect();
    serde_json::json!({"kind": AUTOALLOW_KIND, "tool_name": tool, "tool_input": kept})
}

/// [`run`], and what the hook should print: [`ALLOW_OUTPUT`] only when the daemon answered `ok` to an
/// auto-allow question (it wrote the decision to history first). Any failure: print nothing.
pub fn run_answer(cfg: &Config, stdin: impl Read) -> Result<(Outcome, Option<&'static str>)> {
    // Always drain stdin, even outside Lotse, so the agent never sees a broken pipe.
    let mut raw = Vec::new();
    stdin
        .take(MAX_STDIN_BYTES + 1)
        .read_to_end(&mut raw)
        .context("reading stdin")?;
    let Some(session_id) = cfg.session_id.as_deref().filter(|s| !s.is_empty()) else {
        return Ok((Outcome::NoSession, None));
    };
    if raw.len() as u64 > MAX_STDIN_BYTES {
        return Ok((Outcome::BadInput, None));
    }
    let mut payload: serde_json::Value = match serde_json::from_slice(&raw) {
        Ok(v @ serde_json::Value::Object(_)) => v,
        _ => return Ok((Outcome::BadInput, None)),
    };
    // only the pane's own injected hook asks, and only a Claude one
    let query = payload
        .as_object()
        .filter(|_| cfg.injected && cfg.agent == AgentKind::ClaudeCode)
        .and_then(autoallow_query);
    // Tool output can be huge and no adapter reads it: leave it out of the frame, except the few
    // flags of an Agent call's answer (did the agent launch in the background, under which id).
    if let Some(fields) = payload.as_object_mut() {
        let response = fields.remove("tool_response");
        if let (Some(serde_json::Value::Object(response)), Some(tool)) =
            (response, fields.get("tool_name").and_then(|t| t.as_str()))
            && AGENT_TOOLS.contains(&tool)
        {
            let digest = agent_response_digest(&response);
            fields.insert("tool_response".into(), serde_json::Value::Object(digest));
        }
        if matches!(cfg.agent, AgentKind::ClaudeCode | AgentKind::Codex) {
            // Codex hooks speak the same dialect (`tool_name`, `tool_input`, `prompt`, ...)
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
    let allow = query.and_then(|payload| {
        let ask = Request::HookEvent {
            agent: cfg.agent,
            session_id: Some(SessionId::new(session_id)),
            payload,
        };
        matches!(round_trip(cfg, ask), Ok(Response::Ok)).then_some(ALLOW_OUTPUT)
    });
    Ok((Outcome::Sent, allow))
}

/// Ask the daemon whether `payload` (an [`autoallow_query`]) may skip its prompt: `true` only on `ok`.
pub fn ask_autoallow(cfg: &Config, payload: serde_json::Value) -> bool {
    let Some(session_id) = cfg.session_id.as_deref().filter(|s| !s.is_empty()) else {
        return false;
    };
    let request = Request::HookEvent {
        agent: cfg.agent,
        session_id: Some(SessionId::new(session_id)),
        payload,
    };
    matches!(round_trip(cfg, request), Ok(Response::Ok))
}

/// Fields of a Claude Code hook payload that carry conversation text; the adapter never reads them
/// (but see [`minimize_claude`]: a `Notification`'s `message` and the notification blocks of a
/// prompt are kept in a reduced form).
const CLAUDE_TEXT_FIELDS: [&str; 3] = ["prompt", "last_assistant_message", "message"];

/// The `tool_input` keys the adapter turns into a card summary (`summarize_input` in
/// `lotse-detect/src/claude.rs`), and the two it reads of an Agent call (`subagent_type`,
/// `run_in_background`). Everything else in a tool input (file contents, edits) stays behind.
const CLAUDE_TOOL_INPUT_KEYS: [&str; 10] = [
    "command",
    "file_path",
    "notebook_path",
    "path",
    "url",
    "pattern",
    "query",
    "description",
    "subagent_type",
    "run_in_background",
];

/// The tools that start a sub-agent (`Task` is the older name).
const AGENT_TOOLS: [&str; 2] = ["Agent", "Task"];

/// Longest text kept of a field that is not conversation (a notification's message, a description).
const KEPT_TEXT_CHARS: usize = 200;

/// Data minimisation: the daemon gets what status detection reads, not the user's prompts, replies
/// or the contents of files the agent writes.
fn minimize_claude(fields: &mut serde_json::Map<String, serde_json::Value>) {
    use serde_json::Value;
    let event = fields
        .get("hook_event_name")
        .and_then(Value::as_str)
        .map(str::to_owned);
    let prompt = fields.remove("prompt");
    let message = fields.remove("message");
    for key in CLAUDE_TEXT_FIELDS {
        fields.remove(key);
    }
    // A background agent reports back as a user turn made of `<task-notification>` blocks: they say
    // which agent ended and how, and are not the user's words. Anything else in a prompt is dropped.
    if let Some(digest) = prompt
        .as_ref()
        .and_then(Value::as_str)
        .and_then(notification_digest)
    {
        fields.insert("prompt".into(), Value::String(digest));
    }
    // "Claude needs your permission to use Bash": the tool of a prompt no other hook reports.
    if event.as_deref() == Some("Notification")
        && let Some(Value::String(text)) = message
    {
        let kept: String = text.chars().take(KEPT_TEXT_CHARS).collect();
        fields.insert("message".into(), Value::String(kept));
    }
    // What still runs after a `Stop` (commands and all): only whether, and how many, matter.
    if let Some(tasks) = fields.get_mut("background_tasks") {
        let count = match tasks {
            Value::Array(a) => a.len(),
            Value::Object(o) => o.len(),
            Value::Number(n) => usize::try_from(n.as_u64().unwrap_or(0)).unwrap_or(0),
            _ => 0,
        };
        *tasks = Value::from(count);
    }
    if let Some(Value::Object(input)) = fields.get_mut("tool_input") {
        input.retain(|k, _| CLAUDE_TOOL_INPUT_KEYS.contains(&k.as_str()));
    }
}

/// The flags of an Agent call's answer the adapter reads: the launch status and the agent's ids.
fn agent_response_digest(
    response: &serde_json::Map<String, serde_json::Value>,
) -> serde_json::Map<String, serde_json::Value> {
    use serde_json::Value;
    const KEYS: [&str; 8] = [
        "status",
        "isAsync",
        "is_error",
        "agentId",
        "agent_id",
        "taskId",
        "task_id",
        "description",
    ];
    response
        .iter()
        .filter(|(k, _)| KEYS.contains(&k.as_str()))
        .filter_map(|(k, v)| match v {
            Value::Bool(_) => Some((k.clone(), v.clone())),
            Value::String(s) => Some((
                k.clone(),
                Value::String(s.chars().take(KEPT_TEXT_CHARS).collect()),
            )),
            _ => None,
        })
        .collect()
}

/// `text` reduced to its `<task-notification>` blocks (task id, tool use id, status, summary, and
/// `API Error` as the result when that is how the agent's answer starts), or `None` without one.
fn notification_digest(text: &str) -> Option<String> {
    const OPEN: &str = "<task-notification>";
    const CLOSE: &str = "</task-notification>";
    const MAX_BLOCKS: usize = 64;
    let tag = |block: &str, name: &str| -> Option<String> {
        let open = format!("<{name}>");
        let from = block.find(&open)? + open.len();
        let inner = &block[from..];
        let to = inner.find(&format!("</{name}>")).unwrap_or(inner.len());
        Some(inner[..to].trim().chars().take(KEPT_TEXT_CHARS).collect())
    };
    let mut out = String::new();
    let mut rest = text;
    let mut blocks = 0;
    while blocks < MAX_BLOCKS
        && let Some(start) = rest.find(OPEN)
    {
        let body = &rest[start + OPEN.len()..];
        let end = body.find(CLOSE).unwrap_or(body.len());
        let block = &body[..end];
        out.push_str(OPEN);
        for name in ["task-id", "tool-use-id", "status", "summary"] {
            if let Some(value) = tag(block, name) {
                out.push_str(&format!("<{name}>{value}</{name}>"));
            }
        }
        let api_error = block
            .find("<result>")
            .is_some_and(|at| block[at + 8..].trim_start().starts_with("API Error"));
        if api_error {
            out.push_str("<result>API Error</result>");
        }
        out.push_str(CLOSE);
        out.push('\n');
        rest = &body[end..];
        blocks += 1;
    }
    (blocks > 0).then_some(out)
}

/// Run the exchange on a worker thread so even a connect that hangs cannot outlive the deadline.
fn round_trip(cfg: &Config, request: Request) -> Result<Response> {
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

/// [`round_trip`], where an error answer is an error.
fn deliver(cfg: &Config, request: Request) -> Result<()> {
    match round_trip(cfg, request)? {
        Response::Error(e) => bail!("daemon refused the event: {:?}: {}", e.code, e.message),
        _ => Ok(()),
    }
}

fn exchange(cfg: &Config, request: Request) -> Result<Response> {
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
            })) => return Ok(response),
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

    /// What the daemon receives for `raw`.
    fn forwarded(raw: &[u8]) -> serde_json::Value {
        let d = fake_daemon(Mode::Accept, "t");
        run(&cfg(&d, Some("a1")), raw).unwrap();
        let (_, req) = d.received.recv().unwrap();
        let Request::HookEvent { payload, .. } = req.request else {
            panic!("expected a hook event");
        };
        d.handle.join().unwrap();
        payload
    }

    #[test]
    fn an_agent_calls_launch_flags_survive_and_the_rest_of_its_answer_does_not() {
        let kept = forwarded(
            br#"{"hook_event_name":"PostToolUse","tool_name":"Agent","tool_use_id":"tu9","tool_input":{"description":"Count","prompt":"do it","subagent_type":"general-purpose","run_in_background":true},"tool_response":{"status":"async_launched","agentId":"a1b2","description":"Count","prompt":"the whole brief","outputFile":"/x/y","content":[{"type":"text","text":"big answer"}]}}"#,
        );
        assert_eq!(
            kept,
            serde_json::json!({
                "hook_event_name": "PostToolUse",
                "tool_name": "Agent",
                "tool_use_id": "tu9",
                "tool_input": {"description": "Count", "subagent_type": "general-purpose", "run_in_background": true},
                "tool_response": {"status": "async_launched", "agentId": "a1b2", "description": "Count"},
            })
        );
        // another tool's answer is still dropped whole
        let other = forwarded(
            br#"{"hook_event_name":"PostToolUse","tool_name":"Bash","tool_response":{"status":"ok","stdout":"big"}}"#,
        );
        assert_eq!(
            other,
            serde_json::json!({"hook_event_name": "PostToolUse", "tool_name": "Bash"})
        );
    }

    #[test]
    fn a_report_back_turn_keeps_its_notification_blocks_and_nothing_else() {
        let prompt = "<task-notification>\\n<task-id>a1</task-id>\\n<tool-use-id>toolu_1</tool-use-id>\\n<status>completed</status>\\n<summary>Agent \\\"Count\\\" finished</summary>\\n<result>API Error: 529 secret detail</result>\\n<usage>total_tokens: 9</usage>\\n</task-notification>\\n<task-notification><task-id>a2</task-id><status>completed</status><result>my private answer</result></task-notification>";
        let raw = format!(r#"{{"hook_event_name":"UserPromptSubmit","prompt":"{prompt}"}}"#);
        let kept = forwarded(raw.as_bytes());
        let text = kept["prompt"].as_str().unwrap();
        assert_eq!(
            text,
            "<task-notification><task-id>a1</task-id><tool-use-id>toolu_1</tool-use-id><status>completed</status><summary>Agent \"Count\" finished</summary><result>API Error</result></task-notification>\n<task-notification><task-id>a2</task-id><status>completed</status></task-notification>\n"
        );
        // a user's own prompt never travels, even one that mentions the tag name in passing
        let plain = forwarded(
            br#"{"hook_event_name":"UserPromptSubmit","prompt":"what is a task-notification?"}"#,
        );
        assert_eq!(
            plain,
            serde_json::json!({"hook_event_name": "UserPromptSubmit"})
        );
    }

    #[test]
    fn a_permission_notification_keeps_its_message_and_stop_keeps_only_a_task_count() {
        let kept = forwarded(
            br#"{"hook_event_name":"Notification","notification_type":"permission_prompt","message":"Claude needs your permission to use Bash"}"#,
        );
        assert_eq!(kept["message"], "Claude needs your permission to use Bash");
        // a message of any other event is conversation text
        let other = forwarded(br#"{"hook_event_name":"Stop","message":"note"}"#);
        assert_eq!(other, serde_json::json!({"hook_event_name": "Stop"}));
        let stop = forwarded(
            br#"{"hook_event_name":"Stop","background_tasks":[{"id":"x","command":"npm run dev --token abc"},{"id":"y"}]}"#,
        );
        assert_eq!(stop["background_tasks"], 2);
    }

    #[test]
    fn codex_payloads_leave_conversation_text_and_file_contents_behind() {
        let d = fake_daemon(Mode::Accept, "t");
        let mut cfg = cfg(&d, Some("c1"));
        cfg.agent = AgentKind::Codex;
        let payload = br#"{"session_id":"s1","turn_id":"t1","hook_event_name":"Stop","model":"gpt-5","permission_mode":"default","prompt":"my secret plan","last_assistant_message":"reply text","tool_name":"apply_patch","tool_use_id":"tu1","tool_input":{"command":"*** Begin Patch","content":"fn secret() {}"},"tool_response":{"output":"big"}}"#;
        run(&cfg, &payload[..]).unwrap();
        let (_, req) = d.received.recv().unwrap();
        let Request::HookEvent { agent, payload, .. } = req.request else {
            panic!("expected a hook event");
        };
        assert_eq!(agent, AgentKind::Codex);
        assert_eq!(
            payload,
            serde_json::json!({
                "session_id": "s1",
                "turn_id": "t1",
                "hook_event_name": "Stop",
                "model": "gpt-5",
                "permission_mode": "default",
                "tool_name": "apply_patch",
                "tool_use_id": "tu1",
                "tool_input": {"command": "*** Begin Patch"},
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

    /// Fake daemon of an injected PreToolUse hook: serves `connections` connections (the event, then the
    /// auto-allow question), answering the question as scripted and everything else `ok`.
    fn autoallow_daemon(
        question_answer: Response,
        connections: usize,
    ) -> (tempfile::TempDir, Receiver<Request>) {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("token"), "t\n").unwrap();
        let listener = UnixListener::bind(dir.path().join("lotsed.sock")).unwrap();
        let (tx, received) = mpsc::channel();
        std::thread::spawn(move || {
            for _ in 0..connections {
                let Ok((mut conn, _)) = listener.accept() else {
                    return;
                };
                let Ok(Some(Frame::Control(Message::Hello(_)))) = read_frame(&mut conn) else {
                    return;
                };
                let Ok(Some(Frame::Control(Message::Request(req)))) = read_frame(&mut conn) else {
                    return;
                };
                let is_question = matches!(
                    &req.request,
                    Request::HookEvent { payload, .. } if payload["kind"] == AUTOALLOW_KIND
                );
                let _ = tx.send(req.request.clone());
                let response = if is_question {
                    question_answer.clone()
                } else {
                    Response::Ok
                };
                let mut reply =
                    |m| lotse_proto::write_frame(&mut conn, &Frame::Control(m)).unwrap();
                reply(Message::HelloOk(lotse_proto::HelloOk {
                    proto_version: PROTO_VERSION,
                    daemon_version: "test".into(),
                }));
                reply(Message::Response(ResponseEnvelope {
                    req_id: req.req_id,
                    response,
                }));
            }
        });
        (dir, received)
    }

    const PRE_TOOL: &[u8] = br#"{"hook_event_name":"PreToolUse","permission_mode":"default","tool_name":"Bash","tool_input":{"command":"git status","description":"x","dangerouslyDisableSandbox":false,"timeout":5},"cwd":"/p"}"#;

    fn injected(dir: &tempfile::TempDir) -> Config {
        let mut cfg = Config::in_data_dir(dir.path(), AgentKind::ClaudeCode, Some("a1".into()));
        cfg.deadline = Duration::from_millis(800);
        cfg.injected = true;
        cfg
    }

    fn refused() -> Response {
        Response::Error(ProtoError {
            code: ErrorCode::Conflict,
            message: "no auto-allow rule applies".into(),
        })
    }

    #[test]
    fn the_hook_prints_the_allow_decision_only_when_the_daemon_says_ok() {
        let (dir, received) = autoallow_daemon(Response::Ok, 2);
        let (outcome, answer) = run_answer(&injected(&dir), PRE_TOOL).unwrap();
        assert_eq!((outcome, answer), (Outcome::Sent, Some(ALLOW_OUTPUT)));
        // the event first, untouched by the question; then the question, with what a rule matches on
        let Request::HookEvent { payload: event, .. } = received.recv().unwrap() else {
            panic!("expected the event");
        };
        assert_eq!(event["hook_event_name"], "PreToolUse");
        let Request::HookEvent {
            payload: question,
            session_id,
            ..
        } = received.recv().unwrap()
        else {
            panic!("expected the question");
        };
        assert_eq!(session_id, Some(SessionId::new("a1")));
        assert_eq!(
            question,
            serde_json::json!({
                "kind": AUTOALLOW_KIND,
                "tool_name": "Bash",
                "tool_input": {"command": "git status", "dangerouslyDisableSandbox": false},
            })
        );
        assert!(serde_json::from_str::<serde_json::Value>(ALLOW_OUTPUT).is_ok());

        let (dir, _received) = autoallow_daemon(refused(), 2);
        assert_eq!(
            run_answer(&injected(&dir), PRE_TOOL).unwrap(),
            (Outcome::Sent, None),
            "a refusal prints nothing"
        );
    }

    #[test]
    fn no_answer_to_the_question_is_no_allow() {
        // the daemon serves the event, then is gone: the question cannot be asked, so nothing is allowed
        let (dir, _received) = autoallow_daemon(Response::Ok, 1);
        let started = Instant::now();
        assert_eq!(
            run_answer(&injected(&dir), PRE_TOOL).unwrap(),
            (Outcome::Sent, None)
        );
        assert!(started.elapsed() < Duration::from_secs(3));
    }

    #[test]
    fn only_a_pre_tool_use_of_an_injected_claude_hook_in_default_mode_asks() {
        let no_question = |cfg: Config, raw: &[u8]| {
            let (dir, received) = autoallow_daemon(Response::Ok, 2);
            let mut cfg = cfg;
            cfg.socket = dir.path().join("lotsed.sock");
            cfg.token_file = dir.path().join("token");
            let result = run_answer(&cfg, raw).unwrap();
            assert_eq!(
                result,
                (Outcome::Sent, None),
                "{}",
                String::from_utf8_lossy(raw)
            );
            assert!(
                received.recv().is_ok(),
                "the event itself is still forwarded"
            );
            assert!(
                received.recv_timeout(Duration::from_millis(200)).is_err(),
                "no question"
            );
        };
        let dir = tempfile::tempdir().unwrap();
        let base = injected(&dir);
        let not_injected = Config {
            injected: false,
            ..base.clone()
        };
        let codex = Config {
            agent: AgentKind::Codex,
            ..base.clone()
        };
        no_question(not_injected, PRE_TOOL);
        no_question(codex, PRE_TOOL);
        for mode in [
            "plan",
            "acceptEdits",
            "bypassPermissions",
            "auto",
            "dontAsk",
        ] {
            let raw =
                String::from_utf8_lossy(PRE_TOOL).replace("\"default\"", &format!("\"{mode}\""));
            no_question(base.clone(), raw.as_bytes());
        }
        let no_mode =
            String::from_utf8_lossy(PRE_TOOL).replace("\"permission_mode\":\"default\",", "");
        no_question(base.clone(), no_mode.as_bytes());
        let others: [&[u8]; 4] = [
            br#"{"hook_event_name":"PostToolUse","permission_mode":"default","tool_name":"Bash","tool_input":{"command":"ls"}}"#,
            br#"{"hook_event_name":"PermissionRequest","permission_mode":"default","tool_name":"Bash","tool_input":{"command":"ls"}}"#,
            br#"{"hook_event_name":"PreToolUse","permission_mode":"default","tool_input":{"command":"ls"}}"#,
            br#"{"hook_event_name":"PreToolUse","permission_mode":"default","tool_name":"Bash"}"#,
        ];
        for other in others {
            no_question(base.clone(), other);
        }
    }

    #[test]
    fn the_mod_question_is_built_from_the_tool_and_its_input() {
        let input = serde_json::json!({"command": "ls", "file_path": "/p/a", "content": "secret", "path": "/p"});
        let question = autoallow_question("Bash", input.as_object().unwrap());
        assert_eq!(
            question,
            serde_json::json!({
                "kind": AUTOALLOW_KIND,
                "tool_name": "Bash",
                "tool_input": {"command": "ls", "file_path": "/p/a", "path": "/p"},
            })
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
