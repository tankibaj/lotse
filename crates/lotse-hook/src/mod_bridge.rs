//! The Claude mod's two commands, and the files and gate that go with the mod.
//!
//! ```text
//! lotse-hook mod-bridge    runs for the session's life, started by the mod (`$.process.spawn`)
//! lotse-hook mod-status    one shot, run by the mod (`$.process.run`) to send a batch of status lines
//! ```
//! `mod-bridge` connects to `lotsed` (hello with the token read from its file, client `mod`), attaches
//! to this pane's session (`mod_attach`), prints `{"bridge":"attached"}`, then prints every
//! `Event::ModCommand` the daemon pushes for the session as one JSON line (`{"cmd":"ping"}`).
//! Status lines on its stdin are forwarded like `mod-status`'s. A refused attach (the daemon has no mod
//! support, or knows no such session) makes it exit non-zero and print nothing: the mod then takes the
//! hooks-and-screen path.
//!
//! `mod-status` reads status lines from stdin (`{"t":"heartbeat",...}`, one object per line) and sends
//! each as a `HookEvent` of the session in `LOTSE_SESSION_ID`, over one connection. It exists because
//! `$.process.spawn` closes the child's stdin after the text it is given, so the mod cannot stream
//! status into the long-lived bridge. It prints nothing and exits 0 whatever happens.
//!
//! The payload of a status line is the mod's object plus `kind: "lotse_mod"` and `lotse_source: "mod"`
//! (what the plan names for the daemon to tell them apart). A line can neither name another session nor
//! pose as a hook event: the session id comes from the environment, `hook_event_name` is dropped.
//! No credential travels through the mod: both commands find the socket and the token file the way
//! `lotse-hook` always does ([`Config::from_env`]).

use crate::forward::Config;
use anyhow::{Context, Result, bail};
use lotse_proto::{
    Event, Frame, Hello, Message, PROTO_VERSION, Request, RequestEnvelope, Response,
    ResponseEnvelope, SessionId, encode, read_frame,
};
use serde_json::{Value, json};
use std::io::{BufRead, BufReader, Read, Write};
use std::os::unix::net::UnixStream;
use std::process::ExitCode;
use std::sync::{Arc, Mutex, mpsc};

/// The mod's source, written into the injected plugin as [`MOD_FILE`].
pub const MOD_SOURCE: &str = include_str!("../../../integrations/claude/mod/register.js");

/// Where the mod lives in the plugin directory, and how `hooks/hooks.json` names it.
pub const MOD_FILE: &str = "mod/register.js";
pub const MOD_MODULE_PATH: &str = "../mod/register.js";

/// The mod API is early access: it is loaded only on a Claude Code at least this new.
pub const MIN_CLAUDE_VERSION: &str = "2.1.311";

/// How often the mod reports that it is alive (the daemon treats a longer silence as "mod lost").
pub const HEARTBEAT_SECONDS: u64 = 10;

/// Hello `client` of both commands.
const CLIENT: &str = "mod";

/// Longest status line forwarded, and most that `mod-status` reads in one call.
const MAX_LINE_BYTES: usize = 64 * 1024;
const MAX_SEND_BYTES: u64 = 1024 * 1024;

/// `true` when the `claude --version` output (`2.1.311 (Claude Code)`, or a bare version, or a dev
/// build `2.1.311-dev.…`) names a release of at least [`MIN_CLAUDE_VERSION`].
pub fn mod_supported(version_output: &str) -> bool {
    let min = parse_version(MIN_CLAUDE_VERSION);
    version_output
        .split_whitespace()
        .find_map(parse_version)
        .is_some_and(|v| Some(v) >= min)
}

fn parse_version(word: &str) -> Option<(u32, u32, u32)> {
    let core = word.split(['-', '+']).next()?;
    let mut parts = core.split('.');
    let version = (
        parts.next()?.parse().ok()?,
        parts.next()?.parse().ok()?,
        parts.next()?.parse().ok()?,
    );
    parts.next().is_none().then_some(version)
}

/// The `HookEvent` payload for one status line of the mod, or `None` when the line is not one: an
/// object with a short string `t`, at most [`MAX_LINE_BYTES`] long.
pub fn status_payload(line: &str) -> Option<Value> {
    if line.len() > MAX_LINE_BYTES {
        return None;
    }
    let Value::Object(mut fields) = serde_json::from_str(line.trim()).ok()? else {
        return None;
    };
    match fields.get("t") {
        Some(Value::String(t)) if !t.is_empty() && t.len() <= 32 => {}
        _ => return None,
    }
    for reserved in ["hook_event_name", "session_id", "kind", "lotse_source"] {
        fields.remove(reserved);
    }
    fields.insert("kind".into(), "lotse_mod".into());
    fields.insert("lotse_source".into(), "mod".into());
    Some(Value::Object(fields))
}

fn session(cfg: &Config) -> Option<&str> {
    cfg.session_id.as_deref().filter(|s| !s.is_empty())
}

fn hook_event(cfg: &Config, session: &str, payload: Value) -> Request {
    Request::HookEvent {
        agent: cfg.agent,
        session_id: Some(SessionId::new(session)),
        payload,
    }
}

fn request_frame(req_id: u64, request: Request) -> Result<Vec<u8>> {
    Ok(encode(&Frame::Control(Message::Request(
        RequestEnvelope { req_id, request },
    )))?)
}

/// Connect, then send hello and `requests` (ids 1, 2, …) in one write.
fn open(cfg: &Config, requests: Vec<Request>) -> Result<UnixStream> {
    let token = std::fs::read_to_string(&cfg.token_file)
        .with_context(|| format!("reading {}", cfg.token_file.display()))?
        .trim()
        .to_owned();
    let mut stream = UnixStream::connect(&cfg.socket)
        .with_context(|| format!("connecting to {}", cfg.socket.display()))?;
    // Set once: macOS refuses setsockopt on a socket whose peer already closed (see `forward`).
    let _ = stream.set_write_timeout(Some(cfg.deadline));
    let _ = stream.set_read_timeout(Some(cfg.deadline));
    let mut wire = encode(&Frame::Control(Message::Hello(Hello {
        proto_version: PROTO_VERSION,
        token,
        client: CLIENT.into(),
    })))?;
    for (id, request) in (1..).zip(requests) {
        wire.extend_from_slice(&request_frame(id, request)?);
    }
    stream.write_all(&wire).context("sending to the daemon")?;
    Ok(stream)
}

/// Read until the answers to requests `1..=count` came in; the first error answer fails.
fn await_answers(stream: &mut UnixStream, count: u64) -> Result<()> {
    let mut answered = 0;
    while answered < count {
        let Some(frame) = read_frame(stream).context("reading the daemon's answer")? else {
            bail!("daemon closed the connection without answering");
        };
        // anything else is the hello answer or a pushed event
        if let Frame::Control(Message::Response(ResponseEnvelope { req_id, response })) = frame {
            if let Response::Error(e) = response {
                bail!("daemon refused the request: {:?}: {}", e.code, e.message);
            }
            if (1..=count).contains(&req_id) {
                answered += 1;
            }
        }
    }
    Ok(())
}

/// The long-lived bridge: see the module docs. Returns when the daemon closes the connection or
/// `stdout` closes; an error when the attach is refused or the connection breaks.
pub fn run_bridge(
    cfg: &Config,
    stdin: impl Read + Send + 'static,
    mut stdout: impl Write,
) -> Result<()> {
    let Some(session) = session(cfg) else {
        return Ok(());
    };
    let attach = Request::ModAttach {
        session_id: SessionId::new(session),
    };
    let mut stream = open(cfg, vec![attach])?;
    await_answers(&mut stream, 1).context("mod_attach refused")?;
    // From here on the daemon speaks when it has something to say.
    let _ = stream.set_read_timeout(None);
    writeln!(stdout, "{}", json!({"bridge": "attached"}))?;
    stdout.flush()?;

    let writer = Arc::new(Mutex::new(
        stream.try_clone().context("cloning the socket")?,
    ));
    let (cfg_for_stdin, session_for_stdin) = (cfg.clone(), session.to_owned());
    std::thread::spawn(move || {
        let lines = BufReader::new(stdin).lines().map_while(Result::ok);
        for (id, payload) in (2..).zip(lines.filter_map(|l| status_payload(&l))) {
            let request = hook_event(&cfg_for_stdin, &session_for_stdin, payload);
            let Ok(wire) = request_frame(id, request) else {
                continue;
            };
            let Ok(mut socket) = writer.lock() else { break };
            if socket.write_all(&wire).is_err() {
                break;
            }
        }
    });

    let ours = SessionId::new(session);
    while let Some(frame) = read_frame(&mut stream).context("reading from the daemon")? {
        if let Frame::Control(Message::Event(Event::ModCommand {
            session_id,
            command,
        })) = frame
            && session_id == ours
        {
            writeln!(stdout, "{}", serde_json::to_string(&command)?)?;
            stdout.flush()?;
        }
    }
    Ok(())
}

/// `mod-status`: send every status line of `stdin` over one connection; returns how many were sent
/// (0 outside a Lotse session, or without a valid line). Bounded by `cfg.deadline`.
pub fn run_send(cfg: &Config, stdin: impl Read) -> Result<usize> {
    // Always drain stdin, so the mod's process never sees a broken pipe.
    let mut raw = String::new();
    stdin
        .take(MAX_SEND_BYTES)
        .read_to_string(&mut raw)
        .context("reading stdin")?;
    let Some(session) = session(cfg) else {
        return Ok(0);
    };
    let requests: Vec<Request> = raw
        .lines()
        .filter_map(status_payload)
        .map(|payload| hook_event(cfg, session, payload))
        .collect();
    let count = requests.len();
    if count == 0 {
        return Ok(0);
    }
    // A worker thread, so even a connect that hangs cannot outlive the deadline.
    let (tx, rx) = mpsc::channel();
    let worker_cfg = cfg.clone();
    std::thread::spawn(move || {
        let sent = open(&worker_cfg, requests)
            .and_then(|mut stream| await_answers(&mut stream, count as u64));
        let _ = tx.send(sent);
    });
    match rx.recv_timeout(cfg.deadline) {
        Ok(result) => result.map(|()| count),
        Err(_) => bail!("timed out after {:?}", cfg.deadline),
    }
}

/// Entry of `lotse-hook mod-bridge` (`send_only` false) and `lotse-hook mod-status` (true).
/// `mod-status` never fails; the bridge exits 1 when it could not stay attached.
pub fn main(send_only: bool) -> ExitCode {
    let debug = std::env::var_os("LOTSE_HOOK_DEBUG").is_some();
    let Some(cfg) = Config::from_env(lotse_proto::AgentKind::ClaudeCode, |name| {
        std::env::var_os(name)
    }) else {
        return ExitCode::SUCCESS;
    };
    let result = if send_only {
        run_send(&cfg, std::io::stdin().lock()).map(|_| ())
    } else {
        run_bridge(&cfg, std::io::stdin(), std::io::stdout())
    };
    if let Err(e) = result {
        if debug {
            eprintln!("lotse-hook: {e:#}");
        }
        if !send_only {
            return ExitCode::FAILURE;
        }
    }
    ExitCode::SUCCESS
}
