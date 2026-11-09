//! `lotse-hook mcp`: the `lotse` MCP server an agent starts over stdio, so an agent inside Lotse can
//! start, follow, message and close other Lotse sessions (child sessions).
//!
//! Hand-rolled JSON-RPC 2.0 over stdio (one JSON message per line), `serde_json` only. Six tools:
//! `start_session`, `list_sessions`, `get_session`, `wait_session`, `send_message`, `close_session`.
//! There is no `command` and no `cwd` argument anywhere (that would be arbitrary code execution),
//! and an unknown argument is an error, so the model cannot smuggle one in.
//!
//! The server holds no policy of its own that matters: `lotsed` allows an `mcp:<session>` client
//! only what `lotsed::child_policy` lists (own children and parent, depth and fan-out limits, idle
//! targets for messages) and shows every `start_session` to the user for approval; the tool call
//! blocks until the user answers. `wait_session` polls `list_sessions` (mcp clients get no pushed
//! events). The token is read from its file by [`crate::mcp_link`], never from the environment.

use crate::mcp_link::{Daemon, Link};
use lotse_proto::{
    AgentKind, NewSession, ProjectId, Request, Response, Session, SessionId, Status,
    TranscriptRole, WorktreeSpec,
};
use serde_json::{Map, Value, json};
use std::io::{BufRead, Read, Write};
use std::path::PathBuf;
use std::time::{Duration, Instant};

/// Protocol versions this server speaks, newest first.
const PROTOCOLS: [&str; 3] = ["2025-06-18", "2025-03-26", "2024-11-05"];

const INSTRUCTIONS: &str = "Lotse sessions. Start another session with start_session only when the user asks for one: the full prompt is shown to the user for approval and nothing starts without it. Then follow it with wait_session or get_session. A session can message (send_message) only its own child or parent, and only while that one is idle.";

/// Biggest input line accepted.
const MAX_LINE: u64 = 4 * 1024 * 1024;
/// A tool call that waits for the user (the daemon gives up after 15 minutes).
const START_WAIT: Duration = Duration::from_secs(20 * 60);
const CALL_WAIT: Duration = Duration::from_secs(30);
const DEFAULT_WAIT_S: u64 = 600;
const MAX_WAIT_S: u64 = 3600;
/// What `wait_session` returns of the child's last answer.
const REPORT_CHARS: usize = 400;
/// What `get_session` returns of one transcript turn.
const TURN_CHARS: usize = 2000;
const MAX_TAIL: u64 = 20;
const MAX_TITLE_CHARS: usize = 200;

/// Who this server speaks for: the pane's `LOTSE_SESSION_ID` / `LOTSE_PROJECT_ID`.
#[derive(Debug, Clone, Default)]
pub struct Identity {
    pub session_id: Option<SessionId>,
    pub project_id: Option<ProjectId>,
}

pub const POLL: Duration = Duration::from_secs(1);

pub struct Server<D: Daemon> {
    daemon: D,
    me: Identity,
    poll: Duration,
}

impl<D: Daemon> Server<D> {
    pub fn new(daemon: D, me: Identity) -> Self {
        Server {
            daemon,
            me,
            poll: POLL,
        }
    }

    pub fn with_poll(mut self, poll: Duration) -> Self {
        self.poll = poll;
        self
    }

    /// The answer to one input line, if it needs one (a notification does not).
    pub fn handle_line(&mut self, line: &str) -> Option<String> {
        let Ok(Value::Object(msg)) = serde_json::from_str::<Value>(line) else {
            let parsed = serde_json::from_str::<Value>(line).is_ok();
            let (code, text) = if parsed {
                (-32600, "invalid request")
            } else {
                (-32700, "parse error")
            };
            return Some(rpc_error(&Value::Null, code, text));
        };
        // no `id`: a notification; a `result` or `error` instead of a `method`: an answer to us
        let id = msg.get("id")?.clone();
        let method = msg.get("method").and_then(Value::as_str)?;
        let params = msg.get("params").unwrap_or(&Value::Null);
        Some(match self.dispatch(method, params) {
            Ok(result) => json!({"jsonrpc": "2.0", "id": id, "result": result}).to_string(),
            Err((code, text)) => rpc_error(&id, code, &text),
        })
    }

    pub fn serve(
        &mut self,
        mut input: impl BufRead,
        mut output: impl Write,
    ) -> std::io::Result<()> {
        loop {
            let mut line = Vec::new();
            if (&mut input)
                .take(MAX_LINE + 1)
                .read_until(b'\n', &mut line)?
                == 0
            {
                return Ok(());
            }
            let answer = if line.len() as u64 > MAX_LINE {
                // skip the rest of the oversized line
                while !line.ends_with(b"\n") {
                    line.clear();
                    if (&mut input).take(MAX_LINE).read_until(b'\n', &mut line)? == 0 {
                        break;
                    }
                }
                Some(rpc_error(&Value::Null, -32600, "message too large"))
            } else {
                match std::str::from_utf8(&line).map(str::trim) {
                    Ok("") => None,
                    Ok(text) => self.handle_line(text),
                    Err(_) => Some(rpc_error(&Value::Null, -32700, "parse error")),
                }
            };
            if let Some(answer) = answer {
                writeln!(output, "{answer}")?;
                output.flush()?;
            }
        }
    }

    fn dispatch(&mut self, method: &str, params: &Value) -> Result<Value, (i64, String)> {
        match method {
            "initialize" => {
                let asked = params.get("protocolVersion").and_then(Value::as_str);
                let version = PROTOCOLS
                    .iter()
                    .find(|v| Some(**v) == asked)
                    .unwrap_or(&PROTOCOLS[0]);
                Ok(json!({
                    "protocolVersion": version,
                    "capabilities": {"tools": {}},
                    "serverInfo": {"name": "lotse", "version": env!("CARGO_PKG_VERSION")},
                    "instructions": INSTRUCTIONS,
                }))
            }
            "ping" => Ok(json!({})),
            "tools/list" => Ok(json!({"tools": tool_definitions()})),
            "tools/call" => {
                let name = params
                    .get("name")
                    .and_then(Value::as_str)
                    .ok_or((-32602, "a tool call needs a name".to_owned()))?;
                let args = match params.get("arguments") {
                    None | Some(Value::Null) => Map::new(),
                    Some(Value::Object(args)) => args.clone(),
                    Some(_) => return Err((-32602, "arguments must be an object".into())),
                };
                let outcome = match name {
                    "start_session" => self.start_session(&args),
                    "list_sessions" => self.list_sessions(&args),
                    "get_session" => self.get_session(&args),
                    "wait_session" => self.wait_session(&args),
                    "send_message" => self.send_message(&args),
                    "close_session" => self.close_session(&args),
                    other => return Err((-32602, format!("unknown tool {other:?}"))),
                };
                Ok(match outcome {
                    Ok(value) => {
                        json!({"content": [{"type": "text", "text": value.to_string()}]})
                    }
                    Err(text) => {
                        json!({"content": [{"type": "text", "text": text}], "isError": true})
                    }
                })
            }
            other => Err((-32601, format!("method not found: {other}"))),
        }
    }

    // the daemon

    /// One request to the daemon: its refusals and a broken connection are both tool errors.
    fn ask(&mut self, request: Request, timeout: Duration) -> Result<Response, String> {
        match self.daemon.call(request, timeout) {
            Ok(Response::Error(e)) => Err(e.message),
            Ok(other) => Ok(other),
            Err(e) => Err(format!("cannot reach Lotse: {e}")),
        }
    }

    fn me(&self) -> Result<&SessionId, String> {
        self.me.session_id.as_ref().ok_or_else(|| {
            "not started by Lotse: this agent has no Lotse session (LOTSE_SESSION_ID is not set)"
                .to_owned()
        })
    }

    /// The sessions the daemon shows this one: its project, its children and its parent.
    fn sessions(&mut self) -> Result<Vec<Session>, String> {
        match self.ask(Request::ListSessions { project_id: None }, CALL_WAIT)? {
            Response::Sessions(list) => Ok(list),
            _ => Err("unexpected answer from Lotse".into()),
        }
    }

    fn find(&mut self, id: &str) -> Result<Session, String> {
        self.sessions()?
            .into_iter()
            .find(|s| s.id.as_str() == id)
            .ok_or_else(|| format!("session {id} not found"))
    }

    /// Text of the last assistant turn of `id`, its last [`REPORT_CHARS`] characters.
    fn last_report(&mut self, id: &SessionId) -> Option<String> {
        let request = Request::GetTranscript {
            session_id: id.clone(),
            limit: Some(5),
        };
        let Ok(Response::Transcript(t)) = self.ask(request, CALL_WAIT) else {
            return None;
        };
        let turn = t
            .turns
            .iter()
            .rev()
            .find(|t| t.role == TranscriptRole::Assistant && !t.text.trim().is_empty())?;
        Some(tail_chars(&turn.text, REPORT_CHARS))
    }

    // the tools

    fn start_session(&mut self, args: &Map<String, Value>) -> Result<Value, String> {
        check_keys(args, &["title", "prompt", "agent", "worktree", "project"])?;
        let title = text_arg(args, "title")?;
        if title.chars().count() > MAX_TITLE_CHARS {
            return Err("title: at most 200 characters".into());
        }
        let prompt = text_arg(args, "prompt")?;
        let agent = match opt_str(args, "agent")? {
            None => None,
            Some("claude" | "claude_code") => Some(AgentKind::ClaudeCode),
            Some("codex") => Some(AgentKind::Codex),
            Some("opencode") => Some(AgentKind::OpenCode),
            Some(other) => {
                return Err(format!(
                    "agent: {other:?} is not one of claude, codex, opencode"
                ));
            }
        };
        let worktree = match args.get("worktree") {
            None | Some(Value::Null) => None,
            Some(Value::Object(w)) => {
                check_keys(w, &["branch"]).map_err(|e| format!("worktree: {e}"))?;
                Some(WorktreeSpec {
                    branch: text_arg(w, "branch").map_err(|e| format!("worktree: {e}"))?,
                })
            }
            Some(_) => return Err("worktree: an object with a branch".into()),
        };
        let project = opt_str(args, "project")?.map(str::to_owned);
        let me = self.me()?.clone();
        // the default agent is the parent's own
        let own = match (agent, &project, &self.me.project_id) {
            (Some(_), _, Some(_)) => None,
            _ => self.sessions()?.into_iter().find(|s| s.id == me),
        };
        let agent = agent
            .or_else(|| {
                own.as_ref()
                    .map(|s| s.agent)
                    .filter(|a| *a != AgentKind::Terminal)
            })
            .unwrap_or(AgentKind::ClaudeCode);
        let project_id = project
            .map(ProjectId::new)
            .or_else(|| self.me.project_id.clone())
            .or_else(|| own.map(|s| s.project_id))
            .ok_or("project: no project to start the session in")?;
        let request = Request::CreateSession(NewSession {
            project_id,
            agent,
            command: None,
            title: Some(title),
            worktree,
            first_prompt: Some(prompt),
            parent_session: Some(me),
            requested_by_agent: false,
        });
        match self.ask(request, START_WAIT)? {
            Response::Session(s) => Ok(describe(&s, "session_id")),
            _ => Err("unexpected answer from Lotse".into()),
        }
    }

    fn list_sessions(&mut self, args: &Map<String, Value>) -> Result<Value, String> {
        check_keys(args, &["scope"])?;
        let children = match opt_str(args, "scope")? {
            None | Some("children") => true,
            Some("project") => false,
            Some(other) => return Err(format!("scope: {other:?} is not children or project")),
        };
        let me = self.me()?.clone();
        let sessions = self.sessions()?;
        let project = self.me.project_id.clone().or_else(|| {
            sessions
                .iter()
                .find(|s| s.id == me)
                .map(|s| s.project_id.clone())
        });
        let shown: Vec<Value> = sessions
            .iter()
            .filter(|s| {
                if children {
                    s.parent_session.as_ref() == Some(&me)
                } else {
                    Some(&s.project_id) == project.as_ref()
                }
            })
            .map(|s| describe(s, "id"))
            .collect();
        Ok(json!({"sessions": shown}))
    }

    fn get_session(&mut self, args: &Map<String, Value>) -> Result<Value, String> {
        check_keys(args, &["session_id", "transcript_tail"])?;
        let id = text_arg(args, "session_id")?;
        let tail = match args.get("transcript_tail") {
            None | Some(Value::Null) => None,
            Some(v) => match v.as_u64() {
                Some(n @ 1..=MAX_TAIL) => Some(n as u32),
                _ => return Err("transcript_tail: a number of turns from 1 to 20".into()),
            },
        };
        self.me()?;
        let session = self.find(&id)?;
        let mut out = describe(&session, "session_id");
        if let Some(limit) = tail {
            let request = Request::GetTranscript {
                session_id: session.id.clone(),
                limit: Some(limit),
            };
            let Response::Transcript(t) = self.ask(request, CALL_WAIT)? else {
                return Err("unexpected answer from Lotse".into());
            };
            let turns: Vec<Value> = t
                .turns
                .iter()
                .rev()
                .take(limit as usize)
                .rev()
                .map(|turn| {
                    let text = match (&turn.tool, turn.text.is_empty()) {
                        (Some(tool), true) => format!("{}: {}", tool.name, tool.summary),
                        _ => turn.text.clone(),
                    };
                    json!({"role": turn.role, "text": head_chars(&text, TURN_CHARS)})
                })
                .collect();
            out["transcript_tail"] = Value::Array(turns);
        }
        Ok(out)
    }

    fn wait_session(&mut self, args: &Map<String, Value>) -> Result<Value, String> {
        check_keys(args, &["session_id", "timeout_s"])?;
        let id = text_arg(args, "session_id")?;
        let seconds = match args.get("timeout_s") {
            None | Some(Value::Null) => DEFAULT_WAIT_S,
            Some(v) => v
                .as_u64()
                .ok_or("timeout_s: a number of seconds, 0 or more")?
                .min(MAX_WAIT_S),
        };
        self.me()?;
        let deadline = Instant::now() + Duration::from_secs(seconds);
        loop {
            let found = self.sessions()?.into_iter().find(|s| s.id.as_str() == id);
            let Some(session) = found else {
                return Ok(json!({"session_id": id, "status": "closed"}));
            };
            if matches!(session.status, Status::Done | Status::Fail | Status::Perm) {
                let mut out = describe(&session, "session_id");
                if let Some(report) = self.last_report(&session.id) {
                    out["last_output"] = Value::String(report);
                }
                if let Some(perm) = &session.perm {
                    out["permission"] = json!({"tool": perm.tool, "summary": perm.summary});
                }
                return Ok(out);
            }
            if Instant::now() >= deadline {
                let mut out = describe(&session, "session_id");
                out["timed_out"] = Value::Bool(true);
                return Ok(out);
            }
            std::thread::sleep(self.poll);
        }
    }

    fn send_message(&mut self, args: &Map<String, Value>) -> Result<Value, String> {
        check_keys(args, &["session_id", "text"])?;
        let id = text_arg(args, "session_id")?;
        let text = text_arg(args, "text")?;
        self.me()?;
        self.ask(
            Request::SendInput {
                session_id: SessionId::new(id.clone()),
                text,
            },
            CALL_WAIT,
        )?;
        Ok(json!({"session_id": id, "sent": true}))
    }

    fn close_session(&mut self, args: &Map<String, Value>) -> Result<Value, String> {
        check_keys(args, &["session_id"])?;
        let id = text_arg(args, "session_id")?;
        self.me()?;
        self.ask(
            Request::CloseSession {
                session_id: SessionId::new(id.clone()),
            },
            CALL_WAIT,
        )?;
        Ok(json!({"session_id": id, "closed": true}))
    }
}

fn rpc_error(id: &Value, code: i64, message: &str) -> String {
    json!({"jsonrpc": "2.0", "id": id, "error": {"code": code, "message": message}}).to_string()
}

/// An argument the tool does not declare is an error: a model cannot slip a `command` in.
fn check_keys(args: &Map<String, Value>, allowed: &[&str]) -> Result<(), String> {
    match args.keys().find(|k| !allowed.contains(&k.as_str())) {
        Some(extra) => Err(format!("unknown argument {extra:?}")),
        None => Ok(()),
    }
}

/// A required, non-blank string argument.
fn text_arg(args: &Map<String, Value>, key: &str) -> Result<String, String> {
    match args.get(key) {
        Some(Value::String(s)) if !s.trim().is_empty() => Ok(s.clone()),
        _ => Err(format!("{key}: a non-empty string is required")),
    }
}

fn opt_str<'a>(args: &'a Map<String, Value>, key: &str) -> Result<Option<&'a str>, String> {
    match args.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(s)) => Ok(Some(s)),
        Some(_) => Err(format!("{key}: a string")),
    }
}

fn head_chars(text: &str, n: usize) -> String {
    text.chars().take(n).collect()
}

fn tail_chars(text: &str, n: usize) -> String {
    let skip = text.chars().count().saturating_sub(n);
    text.chars().skip(skip).collect()
}

/// What a tool says about a session; `key` names its id.
fn describe(s: &Session, key: &str) -> Value {
    json!({
        key: s.id,
        "title": s.title,
        "agent": s.agent,
        "status": s.status,
        "project_id": s.project_id,
        "parent": s.parent_session,
    })
}

/// The tool list of `tools/list`. No tool has a `command` or a working directory.
pub fn tool_definitions() -> Value {
    let session_id = json!({"type": "string", "description": "A session id, as returned by start_session or list_sessions."});
    json!([
        {
            "name": "start_session",
            "description": "Start another coding-agent session in Lotse as a child of this one. Use it only when the user asks for it. The full prompt is shown to the user for approval and nothing starts until they approve; the call waits for the answer. Returns the new session's id: follow it with wait_session.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "title": {"type": "string", "description": "Short title of the session."},
                    "prompt": {"type": "string", "description": "The first prompt of the new session. The user reads all of it."},
                    "agent": {"type": "string", "enum": ["claude", "codex", "opencode"], "description": "Which agent to run. Default: the same as this session."},
                    "worktree": {
                        "type": "object",
                        "description": "Work on a new git branch in its own worktree.",
                        "properties": {"branch": {"type": "string", "description": "New branch name, e.g. lotse/fix-login."}},
                        "required": ["branch"],
                        "additionalProperties": false
                    },
                    "project": {"type": "string", "description": "Project id (as in list_sessions). Default: this session's project."}
                },
                "required": ["title", "prompt"],
                "additionalProperties": false
            }
        },
        {
            "name": "list_sessions",
            "description": "List Lotse sessions with their status (run, perm, fail, done, idle, unknown): by default this session's children, or every session of its project.",
            "inputSchema": {
                "type": "object",
                "properties": {"scope": {"type": "string", "enum": ["children", "project"], "description": "Default: children."}},
                "additionalProperties": false
            }
        },
        {
            "name": "get_session",
            "description": "Status of one session and, on request, the last turns of its transcript.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "session_id": session_id,
                    "transcript_tail": {"type": "integer", "minimum": 1, "maximum": 20, "description": "How many of the last transcript turns to return (child or parent sessions only)."}
                },
                "required": ["session_id"],
                "additionalProperties": false
            }
        },
        {
            "name": "wait_session",
            "description": "Wait until a session is done, has failed, or is waiting at a permission prompt, then return its status and the last 400 characters of its answer. Gives up after timeout_s and says so.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "session_id": session_id,
                    "timeout_s": {"type": "integer", "minimum": 0, "maximum": 3600, "description": "Seconds to wait at most. Default 600."}
                },
                "required": ["session_id"],
                "additionalProperties": false
            }
        },
        {
            "name": "send_message",
            "description": "Type a message into one of your own child sessions or into your parent session. It is refused unless that session is idle (done and waiting for input).",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "session_id": session_id,
                    "text": {"type": "string", "description": "The message."}
                },
                "required": ["session_id", "text"],
                "additionalProperties": false
            }
        },
        {
            "name": "close_session",
            "description": "Close one of your own child sessions and end its agent.",
            "inputSchema": {
                "type": "object",
                "properties": {"session_id": session_id},
                "required": ["session_id"],
                "additionalProperties": false
            }
        }
    ])
}

/// Run on the process's stdio. `get` reads the environment.
pub fn run_stdio(get: impl Fn(&str) -> Option<std::ffi::OsString>) -> std::io::Result<()> {
    let non_empty = |name: &str| get(name).filter(|v| !v.is_empty());
    let text = |name: &str| non_empty(name).and_then(|v| v.into_string().ok());
    let me = Identity {
        session_id: text("LOTSE_SESSION_ID").map(SessionId::new),
        project_id: text("LOTSE_PROJECT_ID").map(ProjectId::new),
    };
    let data_dir = crate::forward::data_dir(get(crate::forward::DATA_DIR_ENV).as_deref());
    let socket = non_empty("LOTSE_SOCKET")
        .map(PathBuf::from)
        .or_else(|| data_dir.as_ref().map(|d| d.join("lotsed.sock")))
        .unwrap_or_default();
    let token_file = non_empty("LOTSE_TOKEN_FILE")
        .map(PathBuf::from)
        .or_else(|| data_dir.map(|d| d.join("token")))
        .unwrap_or_default();
    let session = me.session_id.as_ref().map_or("", SessionId::as_str);
    let link = Link::new(socket, token_file, session);
    let stdin = std::io::stdin();
    Server::new(link, me).serve(stdin.lock(), std::io::stdout().lock())
}

#[cfg(test)]
mod tests {
    use super::*;
    use lotse_proto::{
        AgentKind, ErrorCode, GitInfo, NewSession, PermRequest, ProtoError, Session, Status,
        Transcript, TranscriptRole, TranscriptSource, TranscriptTurn, WorktreeSpec,
    };
    use serde_json::json;
    use std::cell::RefCell;
    use std::collections::VecDeque;
    use std::rc::Rc;

    type Log = Rc<RefCell<Vec<Request>>>;
    type Answers = Box<dyn FnMut(&Request) -> Result<Response, String>>;

    struct Fake {
        log: Log,
        answers: Answers,
    }

    impl Daemon for Fake {
        fn call(&mut self, request: Request, _timeout: Duration) -> Result<Response, String> {
            self.log.borrow_mut().push(request.clone());
            (self.answers)(&request)
        }
    }

    fn session(
        id: &str,
        project: &str,
        agent: AgentKind,
        status: Status,
        parent: Option<&str>,
    ) -> Session {
        Session {
            id: SessionId::new(id),
            project_id: ProjectId::new(project),
            title: format!("title of {id}"),
            agent,
            command: "claude".into(),
            status,
            sub_agents: Vec::new(),
            git: GitInfo::default(),
            created_at: 0,
            last_activity: 0,
            running_since: None,
            unread: false,
            perm: None,
            source: lotse_proto::DetectSource::Hook,
            context_pct: None,
            source_layer: None,
            parent_session: parent.map(SessionId::new),
            todo: None,
        }
    }

    fn me() -> Identity {
        Identity {
            session_id: Some(SessionId::new("s-me")),
            project_id: Some(ProjectId::new("p1")),
        }
    }

    fn server(
        me: Identity,
        answers: impl FnMut(&Request) -> Result<Response, String> + 'static,
    ) -> (Server<Fake>, Log) {
        let log = Log::default();
        let fake = Fake {
            log: log.clone(),
            answers: Box::new(answers),
        };
        (
            Server::new(fake, me).with_poll(Duration::from_millis(1)),
            log,
        )
    }

    fn rpc(s: &mut Server<Fake>, msg: Value) -> Value {
        let out = s.handle_line(&msg.to_string()).expect("an answer");
        serde_json::from_str(&out).unwrap()
    }

    /// `tools/call` → (isError, the text of the result).
    fn call(s: &mut Server<Fake>, tool: &str, args: Value) -> (bool, String) {
        let v = rpc(
            s,
            json!({"jsonrpc":"2.0","id":7,"method":"tools/call","params":{"name":tool,"arguments":args}}),
        );
        assert_eq!(v["id"], 7);
        let result = &v["result"];
        assert!(result.is_object(), "{v}");
        (
            result["isError"] == true,
            result["content"][0]["text"].as_str().unwrap().to_owned(),
        )
    }

    fn call_ok(s: &mut Server<Fake>, tool: &str, args: Value) -> Value {
        let (is_error, text) = call(s, tool, args);
        assert!(!is_error, "{text}");
        serde_json::from_str(&text).unwrap()
    }

    fn nothing(_: &Request) -> Result<Response, String> {
        panic!("the daemon must not be called")
    }

    #[test]
    fn initialize_states_the_server_and_when_to_use_it() {
        let (mut s, _) = server(me(), nothing);
        let v = rpc(
            &mut s,
            json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-06-18","capabilities":{},"clientInfo":{"name":"t","version":"1"}}}),
        );
        assert_eq!(v["jsonrpc"], "2.0");
        assert_eq!(v["id"], 1);
        assert_eq!(v["result"]["protocolVersion"], "2025-06-18");
        assert_eq!(v["result"]["serverInfo"]["name"], "lotse");
        assert!(v["result"]["capabilities"]["tools"].is_object());
        let text = v["result"]["instructions"].as_str().unwrap();
        assert!(text.contains("only when the user asks"), "{text}");
        assert!(text.contains("approval"), "{text}");
        // an unknown version gets the newest one this server speaks
        let v = rpc(
            &mut s,
            json!({"jsonrpc":"2.0","id":2,"method":"initialize","params":{"protocolVersion":"1999-01-01"}}),
        );
        assert_eq!(v["result"]["protocolVersion"], "2025-06-18");
    }

    #[test]
    fn notifications_get_no_answer_and_ping_and_unknown_methods_are_handled() {
        let (mut s, _) = server(me(), nothing);
        assert_eq!(
            s.handle_line(r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#),
            None
        );
        assert_eq!(
            s.handle_line(r#"{"jsonrpc":"2.0","method":"anything/unknown"}"#),
            None
        );
        assert_eq!(
            rpc(&mut s, json!({"jsonrpc":"2.0","id":3,"method":"ping"}))["result"],
            json!({})
        );
        let v = rpc(
            &mut s,
            json!({"jsonrpc":"2.0","id":4,"method":"resources/list"}),
        );
        assert_eq!(v["error"]["code"], -32601);
        assert_eq!(v["id"], 4);
        let v: Value = serde_json::from_str(&s.handle_line("{not json").unwrap()).unwrap();
        assert_eq!(v["error"]["code"], -32700);
        assert_eq!(v["id"], Value::Null);
        let v = rpc(
            &mut s,
            json!({"jsonrpc":"2.0","id":5,"method":"tools/call","params":{"name":"rm_rf","arguments":{}}}),
        );
        assert_eq!(v["error"]["code"], -32602);
        let v = rpc(&mut s, json!({"jsonrpc":"2.0","id":"abc","method":"ping"}));
        assert_eq!(v["id"], "abc", "string ids are echoed");
    }

    fn walk(v: &Value, out: &mut Vec<String>) {
        match v {
            Value::Object(m) => {
                for (k, v) in m {
                    out.push(k.clone());
                    walk(v, out);
                }
            }
            Value::Array(a) => a.iter().for_each(|v| walk(v, out)),
            _ => {}
        }
    }

    #[test]
    fn six_tools_and_no_command_or_cwd_anywhere_in_their_schemas() {
        let (mut s, _) = server(me(), nothing);
        let v = rpc(
            &mut s,
            json!({"jsonrpc":"2.0","id":1,"method":"tools/list"}),
        );
        let tools = v["result"]["tools"].as_array().unwrap();
        let names: Vec<&str> = tools.iter().map(|t| t["name"].as_str().unwrap()).collect();
        assert_eq!(
            names,
            [
                "start_session",
                "list_sessions",
                "get_session",
                "wait_session",
                "send_message",
                "close_session"
            ]
        );
        for t in tools {
            assert!(t["description"].as_str().unwrap().len() > 20);
            assert_eq!(t["inputSchema"]["type"], "object");
            assert_eq!(
                t["inputSchema"]["additionalProperties"], false,
                "{}",
                t["name"]
            );
            let mut keys = Vec::new();
            walk(&t["inputSchema"], &mut keys);
            for bad in [
                "command",
                "cmd",
                "cwd",
                "dir",
                "directory",
                "path",
                "env",
                "argv",
                "args",
            ] {
                assert!(!keys.iter().any(|k| k == bad), "{} has `{bad}`", t["name"]);
            }
        }
        let start = &tools[0]["inputSchema"];
        assert_eq!(start["required"], json!(["title", "prompt"]));
        let mut props: Vec<&str> = start["properties"]
            .as_object()
            .unwrap()
            .keys()
            .map(String::as_str)
            .collect();
        props.sort_unstable();
        assert_eq!(props, ["agent", "project", "prompt", "title", "worktree"]);
        assert!(
            tools[0]["description"]
                .as_str()
                .unwrap()
                .contains("approval")
        );
    }

    #[test]
    fn start_session_asks_the_daemon_for_a_child_of_this_session_in_its_project() {
        let (mut s, log) = server(me(), |r| match r {
            Request::ListSessions { .. } => Ok(Response::Sessions(vec![session(
                "s-me",
                "p1",
                AgentKind::Codex,
                Status::Run,
                None,
            )])),
            Request::CreateSession(_) => Ok(Response::Session(session(
                "s-kid",
                "p1",
                AgentKind::Codex,
                Status::Unknown,
                Some("s-me"),
            ))),
            other => panic!("{other:?}"),
        });
        let out = call_ok(
            &mut s,
            "start_session",
            json!({"title":"Fix the test","prompt":"Fix e2e/patients","worktree":{"branch":"lotse/fix"}}),
        );
        assert_eq!(out["session_id"], "s-kid");
        assert_eq!(out["status"], "unknown");
        let sent = log.borrow();
        assert_eq!(
            sent[1],
            Request::CreateSession(NewSession {
                project_id: ProjectId::new("p1"),
                // the default is the parent's own agent
                agent: AgentKind::Codex,
                command: None,
                title: Some("Fix the test".into()),
                worktree: Some(WorktreeSpec {
                    branch: "lotse/fix".into()
                }),
                first_prompt: Some("Fix e2e/patients".into()),
                parent_session: Some(SessionId::new("s-me")),
                requested_by_agent: false,
            })
        );
    }

    #[test]
    fn start_session_agent_and_project_can_be_named_but_a_terminal_cannot() {
        let (mut s, log) = server(me(), |r| match r {
            Request::ListSessions { .. } => Ok(Response::Sessions(Vec::new())),
            _ => Ok(Response::Session(session(
                "s-kid",
                "p2",
                AgentKind::OpenCode,
                Status::Unknown,
                Some("s-me"),
            ))),
        });
        call_ok(
            &mut s,
            "start_session",
            json!({"title":"t","prompt":"p","agent":"opencode","project":"p2"}),
        );
        let Request::CreateSession(ns) = log.borrow().last().unwrap().clone() else {
            panic!("a create_session");
        };
        assert_eq!(
            (ns.agent, ns.project_id.as_str()),
            (AgentKind::OpenCode, "p2")
        );
        // without an own session in the list: Claude Code is the default
        call_ok(&mut s, "start_session", json!({"title":"t","prompt":"p"}));
        let Request::CreateSession(ns) = log.borrow().last().unwrap().clone() else {
            panic!("a create_session");
        };
        assert_eq!(ns.agent, AgentKind::ClaudeCode);
        for agent in ["terminal", "shell", "bash", "claude_code "] {
            let before = log.borrow().len();
            let (is_error, text) = call(
                &mut s,
                "start_session",
                json!({"title":"t","prompt":"p","agent":agent}),
            );
            assert!(is_error, "{agent}: {text}");
            assert_eq!(
                log.borrow().len(),
                before,
                "{agent} never reaches the daemon"
            );
        }
        // the alias of the proto name
        call_ok(
            &mut s,
            "start_session",
            json!({"title":"t","prompt":"p","agent":"claude_code"}),
        );
        call_ok(
            &mut s,
            "start_session",
            json!({"title":"t","prompt":"p","agent":"claude"}),
        );
    }

    #[test]
    fn unknown_arguments_and_missing_ones_are_errors_and_never_reach_the_daemon() {
        let (mut s, log) = server(me(), nothing);
        for (tool, args) in [
            (
                "start_session",
                json!({"title":"t","prompt":"p","command":"rm -rf ~"}),
            ),
            ("start_session", json!({"title":"t","prompt":"p","cwd":"/"})),
            ("start_session", json!({"title":"t"})),
            ("start_session", json!({"prompt":"p"})),
            ("start_session", json!({"title":"t","prompt":"   "})),
            ("start_session", json!({"title":5,"prompt":"p"})),
            (
                "start_session",
                json!({"title":"t","prompt":"p","worktree":{"branch":"b","path":"/x"}}),
            ),
            (
                "start_session",
                json!({"title":"t","prompt":"p","worktree":{}}),
            ),
            ("get_session", json!({})),
            ("wait_session", json!({"session_id":"s-1","timeout_s":-1})),
            (
                "wait_session",
                json!({"session_id":"s-1","timeout_s":"soon"}),
            ),
            ("send_message", json!({"session_id":"s-1"})),
            (
                "send_message",
                json!({"session_id":"s-1","text":"hi","keys":"x"}),
            ),
            ("close_session", json!({"session_id":7})),
            ("list_sessions", json!({"scope":"everything"})),
            (
                "get_session",
                json!({"session_id":"s-1","transcript_tail":99}),
            ),
        ] {
            let (is_error, text) = call(&mut s, tool, args.clone());
            assert!(is_error, "{tool} {args}: {text}");
        }
        // a non-object `arguments`
        let v = rpc(
            &mut s,
            json!({"jsonrpc":"2.0","id":9,"method":"tools/call","params":{"name":"get_session","arguments":[1]}}),
        );
        assert!(
            v["error"].is_object() || v["result"]["isError"] == true,
            "{v}"
        );
        assert!(log.borrow().is_empty());
    }

    #[test]
    fn a_declined_or_refused_start_is_an_error_with_the_daemons_words() {
        let (mut s, _) = server(me(), |r| match r {
            Request::ListSessions { .. } => Ok(Response::Sessions(Vec::new())),
            _ => Ok(Response::Error(ProtoError {
                code: ErrorCode::Conflict,
                message: "the user declined to start the session".into(),
            })),
        });
        let (is_error, text) = call(&mut s, "start_session", json!({"title":"t","prompt":"p"}));
        assert!(is_error);
        assert!(text.contains("declined"), "{text}");
    }

    #[test]
    fn list_sessions_shows_children_by_default_and_the_project_on_request() {
        let all = vec![
            session("s-me", "p1", AgentKind::ClaudeCode, Status::Run, None),
            session("s-kid", "p1", AgentKind::Codex, Status::Done, Some("s-me")),
            session("s-mate", "p1", AgentKind::ClaudeCode, Status::Idle, None),
            session("s-boss", "p9", AgentKind::ClaudeCode, Status::Run, None),
        ];
        let (mut s, log) = server(me(), move |_| Ok(Response::Sessions(all.clone())));
        let out = call_ok(&mut s, "list_sessions", json!({}));
        let ids = |v: &Value| -> Vec<String> {
            v["sessions"]
                .as_array()
                .unwrap()
                .iter()
                .map(|s| s["id"].as_str().unwrap().to_owned())
                .collect()
        };
        assert_eq!(ids(&out), ["s-kid"]);
        assert_eq!(out["sessions"][0]["status"], "done");
        assert_eq!(out["sessions"][0]["agent"], "codex");
        assert_eq!(out["sessions"][0]["parent"], "s-me");
        assert_eq!(out["sessions"][0]["project_id"], "p1");
        assert_eq!(
            ids(&call_ok(
                &mut s,
                "list_sessions",
                json!({"scope":"children"})
            )),
            ["s-kid"]
        );
        assert_eq!(
            ids(&call_ok(
                &mut s,
                "list_sessions",
                json!({"scope":"project"})
            )),
            ["s-me", "s-kid", "s-mate"]
        );
        assert!(
            log.borrow()
                .iter()
                .all(|r| matches!(r, Request::ListSessions { .. }))
        );
    }

    fn turn(role: TranscriptRole, text: &str) -> TranscriptTurn {
        TranscriptTurn {
            role,
            text: text.into(),
            tool: None,
            at: None,
            approval: None,
            sub_agent: None,
        }
    }

    fn transcript(turns: Vec<TranscriptTurn>) -> Response {
        Response::Transcript(Transcript {
            available: true,
            turns,
            truncated: false,
            source: TranscriptSource::Claude,
        })
    }

    #[test]
    fn get_session_gives_the_status_and_on_request_the_tail_of_the_transcript() {
        let (mut s, log) = server(me(), |r| match r {
            Request::ListSessions { .. } => Ok(Response::Sessions(vec![session(
                "s-kid",
                "p1",
                AgentKind::Codex,
                Status::Done,
                Some("s-me"),
            )])),
            Request::GetTranscript { .. } => Ok(transcript(vec![
                turn(TranscriptRole::User, "do it"),
                turn(TranscriptRole::Assistant, &"x".repeat(5000)),
            ])),
            other => panic!("{other:?}"),
        });
        let out = call_ok(&mut s, "get_session", json!({"session_id":"s-kid"}));
        assert_eq!(out["status"], "done");
        assert!(out.get("transcript_tail").is_none());
        let out = call_ok(
            &mut s,
            "get_session",
            json!({"session_id":"s-kid","transcript_tail":2}),
        );
        let tail = out["transcript_tail"].as_array().unwrap();
        assert_eq!(tail.len(), 2);
        assert_eq!(tail[0]["role"], "user");
        assert_eq!(
            tail[1]["text"].as_str().unwrap().chars().count(),
            2000,
            "a turn is cut"
        );
        assert!(log.borrow().iter().any(|r| matches!(
            r,
            Request::GetTranscript { session_id, limit: Some(2) } if session_id.as_str() == "s-kid"
        )));
        let (is_error, text) = call(&mut s, "get_session", json!({"session_id":"s-gone"}));
        assert!(is_error && text.contains("not found"), "{text}");
    }

    #[test]
    fn wait_session_blocks_until_done_and_returns_the_last_400_characters_of_the_report() {
        let polls = Rc::new(RefCell::new(VecDeque::from([
            Status::Run,
            Status::Run,
            Status::Done,
        ])));
        let report: String = ('a'..='z').cycle().take(1000).collect();
        let want_tail: String = report.chars().skip(600).collect();
        let r2 = report.clone();
        let (mut s, log) = server(me(), move |r| match r {
            Request::ListSessions { .. } => {
                let mut q = polls.borrow_mut();
                let status = if q.len() > 1 {
                    q.pop_front().unwrap()
                } else {
                    q[0]
                };
                Ok(Response::Sessions(vec![session(
                    "s-kid",
                    "p1",
                    AgentKind::Codex,
                    status,
                    Some("s-me"),
                )]))
            }
            Request::GetTranscript { .. } => Ok(transcript(vec![
                turn(TranscriptRole::Assistant, "earlier"),
                turn(TranscriptRole::Assistant, &r2),
            ])),
            other => panic!("{other:?}"),
        });
        let out = call_ok(
            &mut s,
            "wait_session",
            json!({"session_id":"s-kid","timeout_s":30}),
        );
        assert_eq!(out["status"], "done");
        assert_eq!(out["last_output"], want_tail);
        assert_eq!(out["last_output"].as_str().unwrap().chars().count(), 400);
        assert!(out.get("timed_out").is_none());
        let lists = log
            .borrow()
            .iter()
            .filter(|r| matches!(r, Request::ListSessions { .. }))
            .count();
        assert_eq!(lists, 3, "one look per poll until it is done");
    }

    #[test]
    fn wait_session_also_returns_at_a_permission_prompt_a_failure_and_a_closed_session() {
        for (status, name) in [(Status::Perm, "perm"), (Status::Fail, "fail")] {
            let (mut s, _) = server(me(), move |r| match r {
                Request::ListSessions { .. } => {
                    let mut kid =
                        session("s-kid", "p1", AgentKind::ClaudeCode, status, Some("s-me"));
                    if status == Status::Perm {
                        kid.perm = Some(PermRequest {
                            tool: "Bash".into(),
                            summary: "npm test".into(),
                            always: None,
                            id: None,
                        });
                    }
                    Ok(Response::Sessions(vec![kid]))
                }
                _ => Ok(transcript(Vec::new())),
            });
            let out = call_ok(&mut s, "wait_session", json!({"session_id":"s-kid"}));
            assert_eq!(out["status"], name);
            if status == Status::Perm {
                assert_eq!(out["permission"]["tool"], "Bash");
                assert_eq!(out["permission"]["summary"], "npm test");
            }
        }
        let (mut s, _) = server(me(), |_| Ok(Response::Sessions(Vec::new())));
        let out = call_ok(&mut s, "wait_session", json!({"session_id":"s-kid"}));
        assert_eq!(out["status"], "closed");
    }

    #[test]
    fn wait_session_gives_up_at_the_timeout_and_says_what_it_saw() {
        let (mut s, _) = server(me(), |_| {
            Ok(Response::Sessions(vec![session(
                "s-kid",
                "p1",
                AgentKind::Codex,
                Status::Run,
                Some("s-me"),
            )]))
        });
        let out = call_ok(
            &mut s,
            "wait_session",
            json!({"session_id":"s-kid","timeout_s":0}),
        );
        assert_eq!(out["status"], "run");
        assert_eq!(out["timed_out"], true);
    }

    #[test]
    fn send_message_and_close_session_pass_through_and_show_the_daemons_refusal() {
        let (mut s, log) = server(me(), |r| match r {
            Request::SendInput { session_id, .. } if session_id.as_str() == "s-busy" => {
                Ok(Response::Error(ProtoError {
                    code: ErrorCode::Conflict,
                    message: "the session is not idle".into(),
                }))
            }
            _ => Ok(Response::Ok),
        });
        call_ok(
            &mut s,
            "send_message",
            json!({"session_id":"s-kid","text":"please also add tests"}),
        );
        let (is_error, text) = call(
            &mut s,
            "send_message",
            json!({"session_id":"s-busy","text":"hi"}),
        );
        assert!(is_error && text.contains("not idle"), "{text}");
        call_ok(&mut s, "close_session", json!({"session_id":"s-kid"}));
        let sent = log.borrow();
        assert_eq!(
            sent[0],
            Request::SendInput {
                session_id: SessionId::new("s-kid"),
                text: "please also add tests".into()
            }
        );
        assert_eq!(
            sent[2],
            Request::CloseSession {
                session_id: SessionId::new("s-kid")
            }
        );
    }

    #[test]
    fn outside_a_lotse_session_every_tool_says_so_and_nothing_is_sent() {
        let (mut s, log) = server(Identity::default(), nothing);
        for (tool, args) in [
            ("start_session", json!({"title":"t","prompt":"p"})),
            ("list_sessions", json!({})),
            ("get_session", json!({"session_id":"s-1"})),
            ("wait_session", json!({"session_id":"s-1"})),
            ("send_message", json!({"session_id":"s-1","text":"x"})),
            ("close_session", json!({"session_id":"s-1"})),
        ] {
            let (is_error, text) = call(&mut s, tool, args);
            assert!(
                is_error && text.contains("not started by Lotse"),
                "{tool}: {text}"
            );
        }
        assert!(log.borrow().is_empty());
        // the protocol itself still answers
        assert!(
            rpc(
                &mut s,
                json!({"jsonrpc":"2.0","id":1,"method":"tools/list"})
            )["result"]["tools"]
                .is_array()
        );
    }

    #[test]
    fn an_unreachable_daemon_is_a_tool_error_not_a_crash() {
        let (mut s, _) = server(me(), |_| Err("cannot connect to /x/lotsed.sock".into()));
        let (is_error, text) = call(&mut s, "list_sessions", json!({}));
        assert!(is_error && text.contains("cannot reach Lotse"), "{text}");
    }

    #[test]
    fn serve_reads_lines_and_writes_one_answer_line_each() {
        let (mut s, _) = server(me(), nothing);
        let input = "{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"ping\"}\n\n{\"jsonrpc\":\"2.0\",\"method\":\"notifications/initialized\"}\n{\"jsonrpc\":\"2.0\",\"id\":2,\"method\":\"ping\"}\n";
        let mut out = Vec::new();
        s.serve(input.as_bytes(), &mut out).unwrap();
        let lines: Vec<&str> = std::str::from_utf8(&out).unwrap().lines().collect();
        assert_eq!(lines.len(), 2, "{lines:?}");
        assert!(lines[0].contains("\"id\":1") && lines[1].contains("\"id\":2"));
    }
}
