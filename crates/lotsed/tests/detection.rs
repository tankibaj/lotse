//! WP8 end to end on the real daemon binary: hooks, OSC 7501, git and worktrees, notifications,
//! and "quit the app, come back: the session is still there with its scrollback".
//! Temp data dir, temp HOME, `--no-login-env`: nothing of the real `~/.claude` or Lotse data is touched.

mod common;

use common::{Client, Daemon, WAIT};
use lotse_proto::{
    AgentKind, ErrorCode, Event, NewSession, NotificationKind, Request, Response, Session, Status,
    SubAgent, WorktreeSpec,
};
use serde_json::json;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

fn agent_session(c: &mut Client, project: &lotse_proto::Project, agent: AgentKind) -> Session {
    match c.request(Request::CreateSession(NewSession {
        project_id: project.id.clone(),
        agent,
        command: Some("/bin/sh".into()),
        title: None,
        worktree: None,
        first_prompt: None,
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

fn status_is(want: Status) -> impl Fn(&Event) -> bool {
    move |e| matches!(e, Event::StatusChanged { status, .. } if *status == want)
}

#[test]
fn hook_events_drive_status_perm_unread_and_notifications() {
    let d = Daemon::start();
    let mut c = d.client();
    let project = c.add_project(&d.project_dir("praxis-web"));
    let s = agent_session(&mut c, &project, AgentKind::ClaudeCode);
    assert_eq!(
        s.status,
        Status::Unknown,
        "an agent session starts without a signal"
    );

    hook(&mut c, &s, json!({"hook_event_name": "UserPromptSubmit"}));
    let Event::StatusChanged {
        running_since,
        source,
        ..
    } = c.wait_event(status_is(Status::Run))
    else {
        unreachable!()
    };
    assert!(running_since.is_some());
    assert_eq!(source, lotse_proto::DetectSource::Hook);

    hook(
        &mut c,
        &s,
        json!({
            "hook_event_name": "PermissionRequest",
            "tool_name": "Bash",
            "tool_input": {"command": "npx playwright test e2e/patients"}
        }),
    );
    let Event::StatusChanged { perm, .. } = c.wait_event(status_is(Status::Perm)) else {
        unreachable!()
    };
    let perm = perm.expect("perm request rides on the event");
    assert_eq!(perm.tool, "Bash");
    assert_eq!(perm.summary, "npx playwright test e2e/patients");

    // the daemon applied the default notification settings (perm: banner + sound)
    let Event::Notification {
        kind,
        title,
        subtitle,
        body,
        sound,
        ..
    } = c.wait_event(|e| matches!(e, Event::Notification { .. }))
    else {
        unreachable!()
    };
    assert_eq!(kind, NotificationKind::Perm);
    assert_eq!(title, "praxis-web · Claude Code");
    assert_eq!(subtitle, "Needs permission");
    assert_eq!(
        body.as_deref(),
        Some("Bash(npx playwright test e2e/patients)")
    );
    assert!(sound);

    // a second request while the first is still open is a new reason to look up: it notifies too
    hook(
        &mut c,
        &s,
        json!({"hook_event_name": "PermissionRequest", "tool_name": "Edit", "tool_input": {"file_path": "src/a.ts"}}),
    );
    let Event::Notification { body, .. } =
        c.wait_event(|e| matches!(e, Event::Notification { .. }))
    else {
        unreachable!()
    };
    assert_eq!(body.as_deref(), Some("Edit(src/a.ts)"));

    hook(
        &mut c,
        &s,
        json!({"hook_event_name": "PreToolUse", "tool_name": "Bash"}),
    );
    c.wait_event(status_is(Status::Run));
    hook(&mut c, &s, json!({"hook_event_name": "Stop"}));
    c.wait_event(status_is(Status::Done));
    let Event::SessionUpdated(updated) = c.wait_event(|e| matches!(e, Event::SessionUpdated(_)))
    else {
        unreachable!()
    };
    assert!(
        updated.unread,
        "a finished turn the user has not seen is unread"
    );
    let Event::Notification { kind, sound, .. } =
        c.wait_event(|e| matches!(e, Event::Notification { .. }))
    else {
        unreachable!()
    };
    assert_eq!(kind, NotificationKind::Done);
    assert!(!sound, "done: banner only by default");

    // the snapshot agrees with the events
    let Response::Snapshot(snap) = c.request(Request::GetSnapshot) else {
        panic!("snapshot")
    };
    let now = snap.sessions.iter().find(|x| x.id == s.id).unwrap();
    assert_eq!(now.status, Status::Done);
    assert!(now.unread);
    assert!(now.running_since.is_none());
}

#[test]
fn sub_agents_flow_through_events_and_a_muted_project_stays_quiet() {
    let d = Daemon::start();
    let mut c = d.client();
    let project = c.add_project(&d.project_dir("quiet"));
    c.request(Request::UpdateProject {
        project_id: project.id.clone(),
        patch: lotse_proto::ProjectPatch {
            notifications: Some(lotse_proto::NotificationMode::Mute),
            ..Default::default()
        },
    });
    let s = agent_session(&mut c, &project, AgentKind::ClaudeCode);
    hook(&mut c, &s, json!({"hook_event_name": "UserPromptSubmit"}));
    hook(
        &mut c,
        &s,
        json!({"hook_event_name": "SubagentStart", "agent_id": "ag-1", "agent_type": "explore-repo"}),
    );
    let Event::SubAgentsChanged { sub_agents, .. } =
        c.wait_event(|e| matches!(e, Event::SubAgentsChanged { .. }))
    else {
        unreachable!()
    };
    assert_eq!(sub_agents.len(), 1);
    assert_eq!(sub_agents[0].name, "explore-repo");
    hook(
        &mut c,
        &s,
        json!({"hook_event_name": "SubagentStop", "agent_id": "ag-1", "agent_type": "explore-repo"}),
    );
    let Event::SubAgentStatus { status, .. } = c.wait_event(|e| {
        matches!(
            e,
            Event::SubAgentStatus {
                status: Status::Done,
                ..
            }
        )
    }) else {
        unreachable!()
    };
    assert_eq!(status, Status::Done);

    hook(
        &mut c,
        &s,
        json!({"hook_event_name": "PermissionRequest", "tool_name": "Bash", "tool_input": {"command": "ls"}}),
    );
    c.wait_event(status_is(Status::Perm));
    c.drain(Duration::from_millis(300));
    assert!(
        !c.events
            .iter()
            .any(|e| matches!(e, Event::Notification { .. })),
        "a muted project never notifies"
    );
    let _: Vec<SubAgent> = sub_agents;
}

#[test]
fn osc_7501_queries_are_answered_on_the_pty() {
    let d = Daemon::start();
    let mut c = d.client();
    let project = c.add_project(&d.project_dir("osc"));
    let s = c.create_session(&project, Some("/bin/sh"));
    c.attach(&s.id, 80, 24);
    c.send_pty(&s.id, b"printf '\\033]7501;?\\033\\\\'; sleep 1\n");
    // the reply is written to the PTY input; the line discipline echoes it back as text
    assert!(c.wait_output(&s.id, "states=idle,working,done,blocked,error"));
}

#[test]
fn the_hook_binary_reaches_the_daemon_with_the_env_the_daemon_exports() {
    let hook_bin = locate("lotse-hook");
    let Some(hook_bin) = hook_bin else {
        panic!(
            "lotse-hook binary not built: run `cargo build -p lotse-hook` (cargo test --workspace builds it)"
        );
    };
    let d = Daemon::start();
    let mut c = d.client();
    let project = c.add_project(&d.project_dir("hookenv"));
    let s = c.create_session(&project, Some("/bin/sh"));
    c.attach(&s.id, 80, 24);
    // no LOTSE_DATA_DIR: the hook must find socket, token file and session id in its environment
    let line = format!(
        "printf '%s' '{{\"hook_event_name\":\"UserPromptSubmit\"}}' | '{}' claude; echo hook-exit-$?\n",
        hook_bin.display()
    );
    c.send_pty(&s.id, line.as_bytes());
    assert!(c.wait_output(&s.id, "hook-exit-0"));
    // a plain shell session: the hook switches detection to Claude and the card goes to Run
    let Event::StatusChanged { status, .. } = c.wait_event(status_is(Status::Run)) else {
        unreachable!()
    };
    assert_eq!(status, Status::Run);
}

#[test]
fn git_line_and_isolated_worktrees() {
    let d = Daemon::start();
    let mut c = d.client();
    let repo = d.project_dir("repo");
    git(&repo, &["init", "-q", "-b", "main"]);
    git(&repo, &["commit", "-q", "--allow-empty", "-m", "init"]);
    std::fs::write(repo.join("a.txt"), "x").unwrap();
    let project = c.add_project(&repo);
    assert!(project.is_git);

    let main = c.create_session(&project, Some("/bin/sh"));
    assert!(main.git.is_git);
    assert_eq!(main.git.branch, "main");
    assert_eq!(main.git.dirty, 1);
    assert!(!main.git.worktree);

    // git_info_changed: a second dirty file shows up within the refresh period
    std::fs::write(repo.join("b.txt"), "y").unwrap();
    let Event::GitInfoChanged { session_id, git } =
        c.wait_event(|e| matches!(e, Event::GitInfoChanged { .. }))
    else {
        unreachable!()
    };
    assert_eq!(session_id, main.id);
    assert_eq!(git.dirty, 2);

    let wt = match c.request(Request::CreateSession(NewSession {
        project_id: project.id.clone(),
        agent: AgentKind::Terminal,
        command: Some("/bin/sh".into()),
        title: None,
        worktree: Some(WorktreeSpec {
            branch: "lotse/new-task".into(),
        }),
        first_prompt: None,
    })) {
        Response::Session(s) => s,
        other => panic!("worktree session: {other:?}"),
    };
    assert!(wt.git.worktree);
    assert_eq!(wt.git.branch, "lotse/new-task");
    let path = wt.git.worktree_path.clone().expect("worktree path");
    assert!(
        Path::new(&path).starts_with(d.home.canonicalize().unwrap()),
        "worktrees live under the (temp) HOME: {path}"
    );
    assert!(Path::new(&path).is_dir());

    // GetGitInfo for that session asks about its worktree, not the project folder
    let Response::GitInfo(info) = c.request(Request::GetGitInfo {
        project_id: project.id.clone(),
        session_id: Some(wt.id.clone()),
    }) else {
        panic!("git info")
    };
    assert!(info.worktree);

    // a bad branch name is refused and leaves nothing behind
    let bad = c.request(Request::CreateSession(NewSession {
        project_id: project.id.clone(),
        agent: AgentKind::Terminal,
        command: Some("/bin/sh".into()),
        title: None,
        worktree: Some(WorktreeSpec {
            branch: "--oops".into(),
        }),
        first_prompt: None,
    }));
    assert!(matches!(bad, Response::Error(e) if e.code == ErrorCode::InvalidRequest));
}

/// The plan's done-when: daemon up, project added, shell session opened, status events flow,
/// the client goes away (app quit) and a new one (app relaunch) finds the session and its scrollback.
#[test]
fn app_relaunch_finds_the_session_with_its_scrollback() {
    let d = Daemon::start();
    let mut first = d.client();
    let project = first.add_project(&d.project_dir("temp-project"));
    let s = first.create_session(&project, Some("/bin/sh"));
    first.attach(&s.id, 80, 24);
    first.send_pty(&s.id, b"echo scrollback-marker-$((6*7))\n");
    assert!(first.wait_output(&s.id, "scrollback-marker-42\r"));
    // events flow to the UI store: the shell is idle (a shell session's status)
    let Response::Snapshot(snap) = first.request(Request::GetSnapshot) else {
        panic!("snapshot")
    };
    assert_eq!(snap.sessions.len(), 1);
    assert_eq!(snap.sessions[0].status, Status::Idle);
    drop(first); // the app quits

    let mut second = d.client(); // the app relaunches
    let Response::Snapshot(snap) = second.request(Request::GetSnapshot) else {
        panic!("snapshot")
    };
    let again = snap
        .sessions
        .iter()
        .find(|x| x.id == s.id)
        .expect("session survived");
    assert_eq!(again.project_id, project.id);
    let replay = second.attach(&s.id, 100, 30);
    assert!(replay > 0, "scrollback is replayed");
    assert!(second.output_text(&s.id).contains("scrollback-marker-42"));
    let _ = WAIT;
}

// ---- helpers ------------------------------------------------------------------------------------

fn git(dir: &Path, args: &[&str]) {
    let ok = Command::new("git")
        .args(args)
        .current_dir(dir)
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_SYSTEM", "/dev/null")
        .env("GIT_AUTHOR_NAME", "t")
        .env("GIT_AUTHOR_EMAIL", "t@t")
        .env("GIT_COMMITTER_NAME", "t")
        .env("GIT_COMMITTER_EMAIL", "t@t")
        .status()
        .unwrap()
        .success();
    assert!(ok, "git {args:?}");
}

/// A workspace binary next to the test executable (`target/<profile>/<name>`).
fn locate(name: &str) -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?;
    let p = exe.parent()?.parent()?.join(name);
    p.is_file().then_some(p)
}
