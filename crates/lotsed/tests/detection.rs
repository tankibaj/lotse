//! WP8 end to end on the real daemon binary: hooks, OSC 7501, git and worktrees, notifications,
//! and "quit the app, come back: the session is still there with its scrollback".
//! Temp data dir, temp HOME, `--no-login-env`: nothing of the real `~/.claude` or Lotse data is touched.

mod common;

use common::{Client, Daemon, WAIT};
use lotse_proto::{
    AgentKind, ErrorCode, Event, NewSession, NotificationKind, PermDecision, Request, Response,
    Session, Status, SubAgent, WorktreeSpec,
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

/// Every `integrations/claude/fixtures/subagents.json` step, sent through the real daemon socket:
/// each change of the sub-agent list arrives as `sub_agents_changed` with the full list.
#[test]
fn claude_subagent_fixture_flows_through_the_daemon_as_full_lists() {
    let fixture: serde_json::Value = serde_json::from_str(
        &std::fs::read_to_string(
            Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../../integrations/claude/fixtures/subagents.json"),
        )
        .unwrap(),
    )
    .unwrap();
    let d = Daemon::start();
    let mut c = d.client();
    let project = c.add_project(&d.project_dir("fixture"));
    let s = agent_session(&mut c, &project, AgentKind::ClaudeCode);
    let mut known = json!([]);
    for step in fixture["steps"].as_array().unwrap() {
        hook(&mut c, &s, step["payload"].clone());
        let expected = &step["sub_agents"];
        if expected != &known {
            let want = expected.clone();
            let Event::SubAgentsChanged { sub_agents, .. } = c.wait_event(
                |e| matches!(e, Event::SubAgentsChanged { sub_agents, .. } if json!(sub_agents) == want),
            ) else {
                unreachable!()
            };
            assert_eq!(json!(sub_agents), *expected);
            known = expected.clone();
        }
    }
}

/// A session that meets its first hook in the middle of a turn (Lotse started late, three
/// background sub-agents already running): the UI only learns of a sub-agent from
/// `sub_agents_changed`, so that event comes before any `sub_agent_status` of the same id, and the
/// status leaves `unknown` through a hook-sourced `status_changed`.
#[test]
fn first_hooks_announce_sub_agents_before_their_status_and_set_a_hook_status() {
    let d = Daemon::start();
    let mut c = d.client();
    let project = c.add_project(&d.project_dir("late"));
    let s = agent_session(&mut c, &project, AgentKind::ClaudeCode);
    for (id, name) in [("a1", "explore"), ("a2", "review"), ("a3", "tests")] {
        hook(
            &mut c,
            &s,
            json!({"hook_event_name": "SubagentStart", "agent_id": id, "agent_type": name}),
        );
    }
    hook(
        &mut c,
        &s,
        json!({"hook_event_name": "SubagentStop", "agent_id": "a2", "agent_type": "review"}),
    );
    c.drain(Duration::from_millis(300));

    let mut announced: Vec<String> = Vec::new();
    let mut statuses = 0;
    let mut hook_status = false;
    for e in &c.events {
        match e {
            Event::SubAgentsChanged { sub_agents, .. } => {
                announced = sub_agents.iter().map(|s| s.id.to_string()).collect();
            }
            Event::SubAgentStatus { sub_agent_id, .. } => {
                statuses += 1;
                assert!(
                    announced.contains(&sub_agent_id.to_string()),
                    "sub_agent_status for {sub_agent_id} before it was announced: {:?}",
                    c.events
                );
            }
            Event::StatusChanged { source, .. } => {
                hook_status |= *source == lotse_proto::DetectSource::Hook;
            }
            _ => {}
        }
    }
    assert_eq!(announced.len(), 3, "the last list holds all three");
    assert!(statuses >= 4, "{statuses}");
    assert!(hook_status, "{:?}", c.events);
}

/// The OpenCode plugin's payloads for a child session (`integrations/opencode/fixtures`), sent
/// through the real daemon socket: the child shows up as a sub-agent with the full list.
#[test]
fn opencode_child_sessions_flow_through_the_daemon_as_sub_agents() {
    let fixture: serde_json::Value = serde_json::from_str(
        &std::fs::read_to_string(
            Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../../integrations/opencode/fixtures/subagent_session.json"),
        )
        .unwrap(),
    )
    .unwrap();
    let d = Daemon::start();
    let mut c = d.client();
    let project = c.add_project(&d.project_dir("opencode"));
    let s = agent_session(&mut c, &project, AgentKind::OpenCode);
    let mut known = json!([]);
    for step in fixture["payloads"].as_array().unwrap() {
        c.expect_ok(Request::HookEvent {
            agent: AgentKind::OpenCode,
            session_id: Some(s.id.clone()),
            payload: step["payload"].clone(),
        });
        let expected = &step["sub_agents"];
        if expected != &known {
            let want = expected.clone();
            c.wait_event(
                |e| matches!(e, Event::SubAgentsChanged { sub_agents, .. } if json!(sub_agents) == want),
            );
            known = expected.clone();
        }
    }
    assert!(
        c.events.iter().any(|e| matches!(
            e,
            Event::StatusChanged {
                source: lotse_proto::DetectSource::Hook,
                ..
            }
        )),
        "{:?}",
        c.events
    );
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

// ResolvePermission types into the pane. The "agent" is a script that paints a prompt, then reports
// the hex of the first byte it reads, so the assertion is on the exact bytes that reached the PTY.

const CLAUDE_PROMPT: &str = " Do you want to proceed?\\n ❯ 1. Yes\\n   2. Yes, allow reading from patients/ from this project\\n   3. Yes, and switch to auto mode (shift+tab)\\n   4. No\\n";
const YES_NO_PROMPT: &str = " Do you want to proceed?\\n ❯ 1. Yes\\n   2. No\\n";

/// A session whose pane shows `prompt` and then waits for one raw byte. The directory holds the
/// script: keep it alive with the session.
fn prompt_pane(
    d: &Daemon,
    c: &mut Client,
    prompt: &str,
    ready: &str,
) -> (Session, tempfile::TempDir) {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("agent.sh");
    common::script(
        dir.path(),
        "agent.sh",
        &format!(
            "stty -icanon -echo min 1\nprintf '{prompt}'\n\
             printf 'BYTES:%s\\n' \"$(dd bs=1 count=1 2>/dev/null | od -An -tx1 | tr -d ' \\n')\"\nsleep 5"
        ),
    );
    let project = c.add_project(&d.project_dir("praxis-web"));
    let s = match c.request(Request::CreateSession(NewSession {
        project_id: project.id.clone(),
        agent: AgentKind::ClaudeCode,
        command: Some(path.to_string_lossy().into_owned()),
        title: None,
        worktree: None,
        first_prompt: None,
    })) {
        Response::Session(s) => s,
        other => panic!("create_session: {other:?}"),
    };
    c.attach(&s.id, 80, 24);
    assert!(c.wait_output(&s.id, ready));
    (s, dir)
}

fn ask_permission(c: &mut Client, s: &Session) {
    hook(
        c,
        s,
        json!({"hook_event_name": "PermissionRequest", "tool_name": "Bash", "tool_input": {"command": "ls"}}),
    );
    c.wait_event(status_is(Status::Perm));
}

fn resolve(c: &mut Client, s: &Session, decision: PermDecision) -> Response {
    c.request(Request::ResolvePermission {
        session_id: s.id.clone(),
        decision,
    })
}

#[test]
fn resolve_permission_types_the_option_digit_the_screen_shows() {
    for (decision, bytes) in [
        (PermDecision::Allow, "BYTES:31"),
        (PermDecision::Always, "BYTES:32"),
        (PermDecision::Deny, "BYTES:34"),
    ] {
        let d = Daemon::start();
        let mut c = d.client();
        let (s, _dir) = prompt_pane(&d, &mut c, CLAUDE_PROMPT, "4. No");
        ask_permission(&mut c, &s);
        assert!(matches!(resolve(&mut c, &s, decision), Response::Ok));
        assert!(c.wait_output(&s.id, bytes), "{decision:?}: wanted {bytes}");
    }
}

#[test]
fn resolve_permission_types_nothing_unless_a_prompt_is_answerable() {
    let (d, d2) = (Daemon::start(), Daemon::start());
    let (mut c, mut c2) = (d.client(), d2.client());

    // nothing on screen: not in perm, and (after the hook says perm) no prompt to read
    let (plain, _dir) = prompt_pane(&d, &mut c, "just output\\n", "just output");
    let conflict =
        |r: Response| matches!(r, Response::Error(ref e) if e.code == ErrorCode::Conflict);
    assert!(conflict(resolve(&mut c, &plain, PermDecision::Allow)));
    ask_permission(&mut c, &plain);
    assert!(conflict(resolve(&mut c, &plain, PermDecision::Deny)));

    // a prompt without "don't ask again": Always is a no-op
    let (yes_no, _dir2) = prompt_pane(&d2, &mut c2, YES_NO_PROMPT, "2. No");
    ask_permission(&mut c2, &yes_no);
    assert!(conflict(resolve(&mut c2, &yes_no, PermDecision::Always)));

    // the first byte each pane reads is the one typed now: nothing was typed before
    for (client, s) in [(&mut c, &plain), (&mut c2, &yes_no)] {
        client.expect_ok(Request::SendInput {
            session_id: s.id.clone(),
            text: "z".into(),
        });
        assert!(client.wait_output(&s.id, "BYTES:7a"));
    }
}
