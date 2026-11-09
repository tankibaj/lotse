//! Beta (wp09): the 1.5 s quiet window, end to end on the real daemon binary. A notification waits;
//! it is dropped when the session left the status that caused it, and repeats collapse into one.
//! Temp data dir, temp HOME, `--no-login-env`: nothing of the real `~/.claude` or Lotse data is touched.

mod common;

use common::{Client, Daemon};
use lotse_proto::{AgentKind, Event, NewSession, Request, Response, Session, Status};
use serde_json::json;
use std::time::{Duration, Instant};

fn session(c: &mut Client, d: &Daemon) -> Session {
    let project = c.add_project(&d.project_dir("quiet-window"));
    match c.request(Request::CreateSession(NewSession {
        project_id: project.id.clone(),
        agent: AgentKind::ClaudeCode,
        command: Some("/bin/sh".into()),
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

fn hook(c: &mut Client, s: &Session, payload: serde_json::Value) {
    c.expect_ok(Request::HookEvent {
        agent: AgentKind::ClaudeCode,
        session_id: Some(s.id.clone()),
        payload,
    });
}

fn ask(c: &mut Client, s: &Session, tool: &str, input: serde_json::Value) {
    hook(
        c,
        s,
        json!({"hook_event_name": "PermissionRequest", "tool_name": tool, "tool_input": input}),
    );
}

fn status_is(want: Status) -> impl Fn(&Event) -> bool {
    move |e| matches!(e, Event::StatusChanged { status, .. } if *status == want)
}

fn is_notification(e: &Event) -> bool {
    matches!(e, Event::Notification { .. })
}

fn notifications(c: &Client) -> usize {
    c.events.iter().filter(|e| is_notification(e)).count()
}

#[test]
fn a_perm_answered_within_the_window_gives_no_notification() {
    let d = Daemon::start();
    let mut c = d.client();
    let s = session(&mut c, &d);
    hook(&mut c, &s, json!({"hook_event_name": "UserPromptSubmit"}));
    c.wait_event(status_is(Status::Run));
    ask(&mut c, &s, "Bash", json!({"command": "ls"}));
    c.wait_event(status_is(Status::Perm));
    // answered (auto-approved, or in the terminal) 300 ms later
    c.drain(Duration::from_millis(300));
    hook(
        &mut c,
        &s,
        json!({"hook_event_name": "PreToolUse", "tool_name": "Bash"}),
    );
    c.wait_event(status_is(Status::Run));
    c.drain(Duration::from_millis(2200));
    assert_eq!(
        notifications(&c),
        0,
        "the agent carried on: no reason to look up"
    );
}

#[test]
fn a_perm_that_stays_notifies_once_after_the_window() {
    let d = Daemon::start();
    let mut c = d.client();
    let s = session(&mut c, &d);
    hook(&mut c, &s, json!({"hook_event_name": "UserPromptSubmit"}));
    c.wait_event(status_is(Status::Run));
    ask(&mut c, &s, "Bash", json!({"command": "ls"}));
    let sent = Instant::now();
    c.wait_event(status_is(Status::Perm));
    assert!(
        !c.pump_until(Duration::from_millis(1100), |c| notifications(c) > 0),
        "held back inside the window"
    );
    c.wait_event(is_notification);
    assert!(
        sent.elapsed() >= Duration::from_millis(1400),
        "went out after {:?}",
        sent.elapsed()
    );
    c.drain(Duration::from_millis(1800));
    assert_eq!(notifications(&c), 0, "exactly one");
}

#[test]
fn repeats_within_the_window_collapse_into_the_latest() {
    let d = Daemon::start();
    let mut c = d.client();
    let s = session(&mut c, &d);
    hook(&mut c, &s, json!({"hook_event_name": "UserPromptSubmit"}));
    c.wait_event(status_is(Status::Run));
    ask(&mut c, &s, "Bash", json!({"command": "ls"}));
    c.drain(Duration::from_millis(200));
    ask(&mut c, &s, "Edit", json!({"file_path": "src/a.ts"}));
    c.drain(Duration::from_millis(200));
    ask(&mut c, &s, "Write", json!({"file_path": "src/b.ts"}));
    let Event::Notification { body, .. } = c.wait_event(is_notification) else {
        unreachable!()
    };
    assert_eq!(body.as_deref(), Some("Write(src/b.ts)"));
    c.drain(Duration::from_millis(1800));
    assert_eq!(notifications(&c), 0, "one per session and kind");
}
