//! Beta (wp08): Session panel: `get_session_info`, `stop_task`.
//!
//! Tasks are the processes in the session leader's process group, minus the leader (the agent or
//! the shell the session started), idle shells and the agent's own program, older than
//! [`MIN_TASK_AGE`]. `stop_task` sends SIGTERM to one pid only if that pid is in this very list: a
//! pid of another group (or of no one) is never signalled.

use crate::launch;
use crate::session::{SessionHandle, now_ms};
use crate::state::{State, unavailable};
use lotse_proto::{
    AgentKind, BackgroundTask, ErrorCode, ProtoError, Request, Response, SessionId, SessionInfo,
};
use nix::errno::Errno;
use nix::sys::signal::{Signal, kill};
use nix::unistd::Pid;
use serde_json::Value;
use std::collections::HashMap;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, SystemTime};

/// A child younger than this is a command that just started, not a background task.
const MIN_TASK_AGE: Duration = Duration::from_secs(10);
const PS_TIMEOUT: Duration = Duration::from_secs(5);
const PS_MAX_BYTES: u64 = 4 * 1024 * 1024;
const MACHINE_TIMEOUT: Duration = Duration::from_secs(3);
/// Bytes read from the end of the transcript to find the model of its last entry.
const MODEL_TAIL_BYTES: u64 = 512 * 1024;
const MAX_COMMAND: usize = 200;

/// Agent versions by binary and its modification time: `--version` of a real agent takes about a
/// second, and the Session panel asks on every open. Only answers are kept, a timeout is retried.
static VERSIONS: OnceLock<Mutex<HashMap<(PathBuf, SystemTime), String>>> = OnceLock::new();

fn cached_version(agent: AgentKind, path: &Path, env: &HashMap<String, String>) -> Option<String> {
    let key = (
        path.to_path_buf(),
        std::fs::metadata(path).ok()?.modified().ok()?,
    );
    let cache = VERSIONS.get_or_init(Mutex::default);
    if let Some(v) = cache.lock().unwrap_or_else(|e| e.into_inner()).get(&key) {
        return Some(v.clone());
    }
    let v = launch::detect_version(agent, path, env)?;
    let mut g = cache.lock().unwrap_or_else(|e| e.into_inner());
    if g.len() >= 64 {
        g.clear();
    }
    g.insert(key, v.clone());
    Some(v)
}
const MAX_MODEL: usize = 100;
const SHELLS: [&str; 8] = ["sh", "bash", "zsh", "fish", "dash", "ksh", "csh", "tcsh"];

pub async fn handle(state: &Arc<State>, req: Request) -> Response {
    match req {
        Request::GetSessionInfo { session_id } => get_info(state, &session_id).await,
        Request::StopTask { session_id, pid } => stop_task(state, &session_id, pid).await,
        other => unavailable(other.method()),
    }
}

fn err(code: ErrorCode, message: impl Into<String>) -> Response {
    Response::Error(ProtoError {
        code,
        message: message.into(),
    })
}

fn not_found() -> Response {
    err(ErrorCode::NotFound, "session not found")
}

/// What the task list is computed from; read off the session before leaving its lock.
struct Group {
    pgid: u32,
    /// Programs that are the agent itself, never a task.
    agent_programs: Vec<String>,
}

fn group_of(h: &SessionHandle) -> Option<Group> {
    let meta = h.snapshot();
    let mut agent_programs = Vec::new();
    agent_programs.extend(program_name(&meta.command));
    if meta.agent != AgentKind::Terminal {
        agent_programs.push(meta.agent.default_command().into());
    }
    Some(Group {
        pgid: h.pgid()?,
        agent_programs,
    })
}

async fn get_info(state: &Arc<State>, session_id: &SessionId) -> Response {
    let Some(h) = state.session(session_id) else {
        return not_found();
    };
    let meta = h.snapshot();
    let cwd = h.cwd();
    let transcript = h.transcript();
    let group = group_of(&h);
    let home = state.config().home.clone();
    let env = state.launch_env().await;
    let configured = launch::configured_command(&state.settings(), meta.agent).to_owned();
    let agent = meta.agent;
    let info = tokio::task::spawn_blocking(move || {
        let agent_version = env
            .tokenize(&configured)
            .ok()
            .and_then(|argv| env.resolve(&argv, &cwd))
            .and_then(|p| cached_version(agent, &p, env.vars()));
        let model = transcript
            .zip(home)
            .and_then(|((_, path), home)| latest_model(&path, &home));
        let tasks = group.and_then(|g| listed_tasks(&g)).unwrap_or_default();
        SessionInfo {
            agent_version,
            model,
            machine: machine_name().to_owned(),
            cwd: cwd.to_string_lossy().into_owned(),
            worktree: meta.git.worktree_path,
            source: meta.source,
            tasks,
        }
    })
    .await;
    match info {
        Ok(info) => Response::SessionInfo(info),
        Err(e) => internal(e),
    }
}

async fn stop_task(state: &Arc<State>, session_id: &SessionId, pid: u32) -> Response {
    let Some(h) = state.session(session_id) else {
        return not_found();
    };
    let group = group_of(&h);
    let res = tokio::task::spawn_blocking(move || {
        // no group (the session's processes are gone) or no process list: nothing is listed
        let listed = group.and_then(|g| listed_tasks(&g)).unwrap_or_default();
        if let (true, Ok(raw)) = (listed.iter().any(|t| t.pid == pid), i32::try_from(pid)) {
            return match kill(Pid::from_raw(raw), Signal::SIGTERM) {
                Ok(()) => Response::Ok,
                Err(Errno::ESRCH) => err(ErrorCode::NotFound, "no such process"),
                Err(e) => err(ErrorCode::Unavailable, format!("signal: {e}")),
            };
        }
        if process_exists(pid) {
            err(
                ErrorCode::InvalidRequest,
                "pid is not a task of this session",
            )
        } else {
            err(ErrorCode::NotFound, "no such process")
        }
    })
    .await;
    res.unwrap_or_else(internal)
}

fn internal(e: impl std::fmt::Display) -> Response {
    tracing::error!("{e}");
    err(ErrorCode::Internal, "internal error")
}

/// Alive (a process we may not signal still exists).
fn process_exists(pid: u32) -> bool {
    let Ok(raw) = i32::try_from(pid) else {
        return false;
    };
    !matches!(kill(Pid::from_raw(raw), None), Err(Errno::ESRCH))
}

/// The group's background tasks older than [`MIN_TASK_AGE`]; `None` when the process list could
/// not be read.
fn listed_tasks(group: &Group) -> Option<Vec<BackgroundTask>> {
    let out = launch::run_capture(
        Path::new("/bin/ps"),
        &["-axww", "-o", "pid=,pgid=,etime=,command="],
        None,
        PS_TIMEOUT,
        PS_MAX_BYTES,
    )?;
    let now = now_ms();
    let tasks: Vec<BackgroundTask> = out
        .lines()
        .filter_map(parse_ps_line)
        .filter(|p| p.pgid == group.pgid && p.pid != group.pgid && p.pid > 1)
        .filter(|p| !is_agent_or_shell(&p.command, &group.agent_programs))
        .filter(|p| p.age > MIN_TASK_AGE)
        .map(|p| BackgroundTask {
            pid: p.pid,
            command: clip(&p.command, MAX_COMMAND),
            started_at: now.saturating_sub(p.age.as_millis() as u64),
        })
        .collect();
    Some(tasks)
}

struct Proc {
    pid: u32,
    pgid: u32,
    age: Duration,
    command: String,
}

/// One `ps -o pid=,pgid=,etime=,command=` line.
fn parse_ps_line(line: &str) -> Option<Proc> {
    let mut rest = line.trim_start();
    let mut field = || {
        let end = rest.find(char::is_whitespace)?;
        let (head, tail) = rest.split_at(end);
        rest = tail.trim_start();
        Some(head)
    };
    let pid = field()?.parse().ok()?;
    let pgid = field()?.parse().ok()?;
    let age = parse_etime(field()?)?;
    Some(Proc {
        pid,
        pgid,
        age,
        command: rest.trim_end().to_owned(),
    })
}

/// `ps` elapsed time: `[[dd-]hh:]mm:ss`.
fn parse_etime(s: &str) -> Option<Duration> {
    let (days, clock) = match s.split_once('-') {
        Some((d, c)) => (d.parse::<u64>().ok()?, c),
        None => (0, s),
    };
    let parts: Vec<u64> = clock
        .split(':')
        .map(|p| p.parse().ok())
        .collect::<Option<_>>()?;
    let secs = match parts[..] {
        [m, s] => m * 60 + s,
        [h, m, s] => (h * 60 + m) * 60 + s,
        _ => return None,
    };
    Some(Duration::from_secs(days * 86_400 + secs))
}

/// Basename of the first word of a command line, a login shell's leading `-` removed.
fn program_name(command: &str) -> Option<String> {
    let first = command.split_whitespace().next()?;
    let name = Path::new(first).file_name()?.to_string_lossy();
    Some(name.trim_start_matches('-').to_owned())
}

/// The agent's own program, or a shell that was not given a script or command to run.
fn is_agent_or_shell(command: &str, agent_programs: &[String]) -> bool {
    let Some(name) = program_name(command) else {
        return true;
    };
    if agent_programs.contains(&name) {
        return true;
    }
    SHELLS.contains(&name.as_str())
        && command
            .split_whitespace()
            .skip(1)
            .all(|a| a.starts_with('-'))
}

fn clip(s: &str, max: usize) -> String {
    s.chars().take(max).collect()
}

/// `scutil --get ComputerName`, once; the host name when scutil has none.
fn machine_name() -> &'static str {
    static NAME: OnceLock<String> = OnceLock::new();
    NAME.get_or_init(|| {
        let ask = |program: &str, args: &[&str]| {
            launch::run_capture(Path::new(program), args, None, MACHINE_TIMEOUT, 1024)
                .map(|s| s.trim().to_owned())
                .filter(|s| !s.is_empty())
        };
        ask("/usr/sbin/scutil", &["--get", "ComputerName"])
            .or_else(|| ask("/bin/hostname", &[]))
            .unwrap_or_else(|| "unknown".into())
    })
}

/// The model named by the newest entry of the transcript that names one. Claude Code puts it at
/// `message.model` of assistant entries; Codex (**Unverified**, shaped from its rollout format) at
/// `payload.model` of `turn_context` entries. Placeholders like `<synthetic>` do not count.
fn latest_model(path: &Path, home: &Path) -> Option<String> {
    let real = crate::transcript::validate_path(&path.to_string_lossy(), home)?;
    let mut file = std::fs::File::open(real).ok()?;
    let len = file.metadata().ok()?.len();
    let start = len.saturating_sub(MODEL_TAIL_BYTES);
    file.seek(SeekFrom::Start(start)).ok()?;
    let mut bytes = Vec::new();
    file.take(MODEL_TAIL_BYTES).read_to_end(&mut bytes).ok()?;
    let text = String::from_utf8_lossy(&bytes);
    // a tail starts mid-line: drop the partial first line
    let text = if start > 0 {
        text.split_once('\n').map_or("", |(_, rest)| rest)
    } else {
        &text
    };
    text.lines().rev().find_map(|line| {
        let v: Value = serde_json::from_str(line).ok()?;
        let model = [&v["message"]["model"], &v["payload"]["model"], &v["model"]]
            .into_iter()
            .find_map(Value::as_str)?;
        (!model.is_empty() && !model.starts_with('<')).then(|| clip(model, MAX_MODEL))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn etime_formats() {
        let secs = |s| parse_etime(s).map(|d| d.as_secs());
        assert_eq!(secs("00:07"), Some(7));
        assert_eq!(secs("12:34"), Some(754));
        assert_eq!(secs("01:02:03"), Some(3723));
        assert_eq!(secs("2-01:02:03"), Some(2 * 86_400 + 3723));
        assert_eq!(secs("junk"), None);
        assert_eq!(secs("1:2:3:4"), None);
    }

    #[test]
    fn ps_lines_keep_the_whole_command() {
        let p = parse_ps_line("  4242  4200    01:30 sleep   60 &").unwrap();
        assert_eq!((p.pid, p.pgid, p.age.as_secs()), (4242, 4200, 90));
        assert_eq!(p.command, "sleep   60 &");
        assert!(parse_ps_line("not a process line").is_none());
    }

    #[test]
    fn the_agent_and_idle_shells_are_not_tasks() {
        let agent = vec!["claude".to_owned()];
        assert!(is_agent_or_shell("/usr/local/bin/claude --resume", &agent));
        assert!(is_agent_or_shell("-zsh", &agent));
        assert!(is_agent_or_shell("/bin/bash -l", &agent));
        assert!(!is_agent_or_shell("bash build.sh", &agent));
        assert!(!is_agent_or_shell("sleep 60", &agent));
        assert!(!is_agent_or_shell("npm run dev", &agent));
    }
}
