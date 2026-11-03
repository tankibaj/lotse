//! Session panel on the real daemon: `get_session_info` and `stop_task`. Temp data dir and HOME;
//! the children are `sleep`s of a fake agent script, never a real agent.

mod common;

use common::{Client, Daemon};
use lotse_proto::{
    AgentKind, DetectSource, ErrorCode, NewSession, Request, Response, Session, SessionInfo,
};
use std::time::{Duration, Instant};

/// The session's process: starts a background `sleep` in its own process group (a script has no job
/// control) and prints the pids.
const AGENT: &str = "echo \"agent=$$\"\n\
                     sleep 60 &\n\
                     echo \"task=$!\"\n\
                     wait";

/// The task list needs a child older than this (the daemon's own threshold, 10 s) plus slack.
const TASK_WAIT: Duration = Duration::from_secs(25);

struct Fixture {
    d: Daemon,
    c: Client,
    session: Session,
    task_pid: i32,
    agent_pid: i32,
    _scripts: tempfile::TempDir,
    _bin: tempfile::TempDir,
}

fn start() -> Fixture {
    // launch detection runs the configured `claude --version`: a fake one answers
    let bin = tempfile::tempdir().unwrap();
    common::script(bin.path(), "claude", "echo '2.1.309 (Claude Code)'");
    let d = Daemon::start_with_path(Some(bin.path()));
    let mut c = d.client();
    let scripts = tempfile::tempdir().unwrap();
    common::script(scripts.path(), "claude-fake.sh", AGENT);
    let project = c.add_project(&d.project_dir("praxis-web"));
    let session = match c.request(Request::CreateSession(NewSession {
        project_id: project.id.clone(),
        agent: AgentKind::ClaudeCode,
        command: Some(
            scripts
                .path()
                .join("claude-fake.sh")
                .to_string_lossy()
                .into_owned(),
        ),
        title: None,
        worktree: None,
        first_prompt: None,
    })) {
        Response::Session(s) => s,
        other => panic!("create_session: {other:?}"),
    };
    c.attach(&session.id, 80, 24);
    let task_pid = c.wait_number(&session.id, "task=");
    let agent_pid = c.wait_number(&session.id, "agent=");
    Fixture {
        d,
        c,
        session,
        task_pid,
        agent_pid,
        _scripts: scripts,
        _bin: bin,
    }
}

fn info(c: &mut Client, f: &Session) -> SessionInfo {
    match c.request(Request::GetSessionInfo {
        session_id: f.id.clone(),
    }) {
        Response::SessionInfo(i) => i,
        other => panic!("get_session_info: {other:?}"),
    }
}

/// The info once the child is old enough to be listed.
fn info_with_task(fx: &mut Fixture) -> SessionInfo {
    let start = Instant::now();
    loop {
        let i = info(&mut fx.c, &fx.session);
        if i.tasks.iter().any(|t| t.pid as i32 == fx.task_pid) {
            return i;
        }
        assert!(start.elapsed() < TASK_WAIT, "task never listed: {i:?}");
        std::thread::sleep(Duration::from_millis(250));
    }
}

fn error_code(r: &Response) -> ErrorCode {
    match r {
        Response::Error(e) => e.code,
        other => panic!("expected an error, got {other:?}"),
    }
}

fn close(fx: &mut Fixture) {
    fx.c.expect_ok(Request::CloseSession {
        session_id: fx.session.id.clone(),
    });
}

#[test]
fn lists_background_task() {
    let mut fx = start();
    // a task younger than 10 s is not one yet
    assert!(
        info(&mut fx.c, &fx.session).tasks.is_empty(),
        "a fresh child is listed"
    );
    let i = info_with_task(&mut fx);
    let task = i
        .tasks
        .iter()
        .find(|t| t.pid as i32 == fx.task_pid)
        .unwrap();
    assert!(task.command.contains("sleep 60"), "{task:?}");
    assert!(task.started_at > 0);
    // the agent (the group leader) is not a task
    assert_eq!(i.tasks.len(), 1, "{:?}", i.tasks);
    close(&mut fx);
}

#[test]
fn info_fields_are_set() {
    let mut fx = start();
    let i = info(&mut fx.c, &fx.session);
    assert_eq!(i.agent_version.as_deref(), Some("v2.1.309"));
    assert_eq!(i.model, None, "no transcript announced yet");
    assert!(!i.machine.is_empty());
    assert!(i.cwd.ends_with("praxis-web"), "{}", i.cwd);
    assert_eq!(i.worktree, None);
    assert_eq!(i.source, DetectSource::Process);
    close(&mut fx);
}

#[test]
fn model_comes_from_the_latest_transcript_entry() {
    let mut fx = start();
    let dir = fx.d.home.join(".claude/projects/x");
    std::fs::create_dir_all(&dir).unwrap();
    let transcript = dir.join("t.jsonl");
    std::fs::write(
        &transcript,
        "{\"type\":\"assistant\",\"message\":{\"model\":\"claude-opus-4-1\",\"content\":[]}}\n\
         {\"type\":\"assistant\",\"message\":{\"model\":\"claude-sonnet-4-5\",\"content\":[]}}\n\
         {\"type\":\"user\",\"message\":{\"content\":\"hi\"}}\n\
         {\"type\":\"assistant\",\"message\":{\"model\":\"<synthetic>\",\"content\":[]}}\n",
    )
    .unwrap();
    fx.c.expect_ok(Request::HookEvent {
        agent: AgentKind::ClaudeCode,
        session_id: Some(fx.session.id.clone()),
        payload: serde_json::json!({
            "hook_event_name": "UserPromptSubmit",
            "transcript_path": transcript.to_string_lossy(),
        }),
    });
    assert_eq!(
        info(&mut fx.c, &fx.session).model.as_deref(),
        Some("claude-sonnet-4-5")
    );
    close(&mut fx);
}

#[test]
fn stop_task_kills_child() {
    let mut fx = start();
    info_with_task(&mut fx);
    assert!(common::pid_alive(fx.task_pid));
    fx.c.expect_ok(Request::StopTask {
        session_id: fx.session.id.clone(),
        pid: fx.task_pid as u32,
    });
    assert!(common::wait_dead(fx.task_pid), "the task survived SIGTERM");
    // gone: not found, and never a signal to whoever got the number since
    let again = fx.c.request(Request::StopTask {
        session_id: fx.session.id.clone(),
        pid: fx.task_pid as u32,
    });
    assert_eq!(error_code(&again), ErrorCode::NotFound, "{again:?}");
    close(&mut fx);
}

#[test]
fn stop_task_refuses_foreign_pid() {
    let mut fx = start();
    info_with_task(&mut fx);
    // live processes outside the session's task list: a stranger, the daemon, this test, init and
    // the agent itself (the session's own leader is not a task)
    let mut outsider = std::process::Command::new("sleep")
        .arg("30")
        .spawn()
        .unwrap();
    let outsider_pid = outsider.id();
    for pid in [
        outsider_pid,
        fx.d.pid(),
        std::process::id(),
        1,
        fx.agent_pid as u32,
    ] {
        let r = fx.c.request(Request::StopTask {
            session_id: fx.session.id.clone(),
            pid,
        });
        assert_eq!(
            error_code(&r),
            ErrorCode::InvalidRequest,
            "pid {pid}: {r:?}"
        );
    }
    assert!(
        common::pid_alive(outsider_pid as i32),
        "foreign pid signalled"
    );
    assert!(common::pid_alive(fx.agent_pid), "the agent was signalled");
    assert!(common::pid_alive(fx.task_pid), "the session's task was hit");
    let _ = outsider.kill();
    let _ = outsider.wait();
    close(&mut fx);
}

#[test]
fn unknown_session_is_not_found() {
    let d = Daemon::start();
    let mut c = d.client();
    let session_id = lotse_proto::SessionId::new("nope");
    let r = c.request(Request::GetSessionInfo {
        session_id: session_id.clone(),
    });
    assert_eq!(error_code(&r), ErrorCode::NotFound, "{r:?}");
    let r = c.request(Request::StopTask { session_id, pid: 1 });
    assert_eq!(error_code(&r), ErrorCode::NotFound, "{r:?}");
}
