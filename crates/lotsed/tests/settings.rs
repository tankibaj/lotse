//! Settings, launch commands (overrides, defaults, PATH lookup, version detection).

mod common;

use common::*;
use lotse_proto::{AgentKind, ErrorCode, Event, LaunchCommandInfo, Request, Response, Settings};
use std::time::{Duration, Instant};

fn settings(c: &mut Client) -> Settings {
    match c.request(Request::GetSettings) {
        Response::Settings(s) => s,
        other => panic!("{other:?}"),
    }
}

fn row(rows: &[LaunchCommandInfo], agent: AgentKind) -> &LaunchCommandInfo {
    rows.iter().find(|r| r.agent == agent).unwrap()
}

#[test]
fn defaults_match_the_mock_and_updates_broadcast() {
    let d = Daemon::start();
    let mut a = d.client();
    let mut b = d.client();
    let s = settings(&mut a);
    assert_eq!(s, Settings::default());
    assert_eq!(s.launch_commands.terminal, "$SHELL -l");
    assert_eq!(s.launch_commands.claude_code, "claude");
    assert_eq!(s.scrollback_bytes, 4 * 1024 * 1024);

    let mut next = s.clone();
    next.launch_commands.claude_code = "  claude --model opus ".into();
    next.launch_commands.opencode = "".into(); // empty = reset to default
    next.notifications.paused = true;
    let Response::Settings(saved) = a.request(Request::UpdateSettings { settings: next }) else {
        panic!("update failed")
    };
    assert_eq!(saved.launch_commands.claude_code, "claude --model opus");
    assert_eq!(saved.launch_commands.opencode, "opencode");
    assert!(saved.notifications.paused);
    let ev = b.wait_event(|e| matches!(e, Event::SettingsChanged(_)));
    assert!(matches!(ev, Event::SettingsChanged(s) if s == saved));
    assert_eq!(settings(&mut b), saved);
}

#[test]
fn invalid_settings_are_rejected_and_change_nothing() {
    let d = Daemon::start();
    let mut c = d.client();
    let good = settings(&mut c);
    for mutate in [
        (|s: &mut Settings| s.launch_commands.codex = "codex 'oops".into()) as fn(&mut Settings),
        |s| s.scrollback_bytes = 10,
        |s| s.scrollback_bytes = u64::MAX,
    ] {
        let mut bad = good.clone();
        mutate(&mut bad);
        match c.request(Request::UpdateSettings { settings: bad }) {
            Response::Error(e) => assert_eq!(e.code, ErrorCode::InvalidRequest),
            other => panic!("{other:?}"),
        }
    }
    assert_eq!(settings(&mut c), good);
}

#[test]
fn launch_command_detection_finds_versions_and_times_out_after_three_seconds() {
    let bin = tempfile::Builder::new().prefix("lb").tempdir().unwrap();
    script(bin.path(), "claude", r#"echo "2.1.309 (Claude Code)""#);
    script(bin.path(), "codex", "sleep 20"); // hangs: found, but no version
    let d = Daemon::start_with_path(Some(bin.path()));
    let mut c = d.client();

    let started = Instant::now();
    let Response::LaunchCommands(rows) = c.request(Request::DetectLaunchCommands) else {
        panic!("no launch commands")
    };
    let took = started.elapsed();
    assert_eq!(rows.len(), 4);

    let term = row(&rows, AgentKind::Terminal);
    assert_eq!(term.default_command, "$SHELL -l");
    assert_eq!(term.command, "$SHELL -l");
    assert!(term.found);
    assert_eq!(term.resolved_path.as_deref(), Some("/bin/sh"));
    assert!(
        term.version
            .as_deref()
            .is_some_and(|v| v.starts_with("sh ")),
        "{:?}",
        term.version
    );

    let claude = row(&rows, AgentKind::ClaudeCode);
    assert!(claude.found);
    assert_eq!(claude.version.as_deref(), Some("v2.1.309"));
    assert_eq!(
        claude.resolved_path.as_deref(),
        Some(bin.path().join("claude").to_str().unwrap())
    );
    assert_eq!(claude.default_command, "claude");

    let codex = row(&rows, AgentKind::Codex);
    assert!(codex.found, "on PATH even though its --version hangs");
    assert_eq!(codex.version, None);

    let opencode = row(&rows, AgentKind::OpenCode);
    assert!(!opencode.found, "not on PATH renders `not found`");
    assert_eq!(opencode.resolved_path, None);
    assert_eq!(opencode.version, None);

    assert!(
        took >= Duration::from_millis(2900),
        "the timeout is 3 s, took {took:?}"
    );
    assert!(
        took < Duration::from_secs(8),
        "probes run in parallel, took {took:?}"
    );
}

#[test]
fn configured_override_is_what_gets_detected_and_launched() {
    let bin = tempfile::Builder::new().prefix("lb").tempdir().unwrap();
    script(
        bin.path(),
        "my-claude",
        r#"if [ "$1" = "--version" ]; then echo "v9.9.9"; else echo launched-with:"$@"; sleep 30; fi"#,
    );
    let d = Daemon::start_with_path(Some(bin.path()));
    let mut c = d.client();
    let mut s = settings(&mut c);
    s.launch_commands.claude_code = "my-claude --model \"opus 4\"".into();
    assert!(matches!(
        c.request(Request::UpdateSettings { settings: s }),
        Response::Settings(_)
    ));

    let Response::LaunchCommands(rows) = c.request(Request::DetectLaunchCommands) else {
        panic!()
    };
    let claude = row(&rows, AgentKind::ClaudeCode);
    assert!(claude.found);
    assert_eq!(claude.version.as_deref(), Some("v9.9.9"));
    assert_eq!(claude.command, "my-claude --model \"opus 4\"");
    assert_eq!(
        claude.default_command, "claude",
        "default stays the Reset target"
    );

    // a session of that agent starts the override, args split like a shell would
    let project = c.add_project(&d.project_dir("proj"));
    let Response::Session(session) = c.request(Request::CreateSession(lotse_proto::NewSession {
        project_id: project.id.clone(),
        agent: AgentKind::ClaudeCode,
        command: None,
        title: None,
        worktree: None,
        first_prompt: None,
    })) else {
        panic!("create failed")
    };
    assert_eq!(session.command, "my-claude --model \"opus 4\"");
    assert_eq!(session.status, lotse_proto::Status::Unknown);
    c.attach(&session.id, 80, 24);
    assert!(
        c.wait_output(&session.id, "launched-with:--model opus 4"),
        "{:?}",
        c.output_text(&session.id)
    );

    // per-session override beats the setting
    let other = c.create_session(&project, Some("/bin/sh"));
    assert_eq!(other.command, "/bin/sh");
}

/// Production path: the environment comes from `$SHELL -l -i -c …`. A fake shell stands in for zsh.
#[test]
fn sessions_run_in_the_login_shell_environment() {
    let bin = tempfile::Builder::new().prefix("lb").tempdir().unwrap();
    let tools = bin.path().join("tools");
    std::fs::create_dir_all(&tools).unwrap();
    script(&tools, "claude", r#"echo "3.4.5 (Claude Code)""#);
    // like `zsh -l -i -c`: add what .zprofile / .zshrc would export, then run the script
    script(
        bin.path(),
        "fakeshell",
        &format!(
            r#"[ "$1" = "-l" ] && [ "$2" = "-i" ] && [ "$3" = "-c" ] || exit 64
export LOTSE_FROM_LOGIN=yes
export PATH="{}:$PATH"
exec /bin/sh -c "$4""#,
            tools.display()
        ),
    );
    let shell = bin.path().join("fakeshell");
    let d = Daemon::start_with_login_shell(&shell);
    let mut c = d.client();

    // PATH from the login shell makes `claude` detectable although the daemon's own PATH lacks it
    let Response::LaunchCommands(rows) = c.request(Request::DetectLaunchCommands) else {
        panic!()
    };
    let claude = row(&rows, AgentKind::ClaudeCode);
    assert!(claude.found, "{claude:?}");
    assert_eq!(claude.version.as_deref(), Some("v3.4.5"));
    let term = row(&rows, AgentKind::Terminal);
    assert_eq!(
        term.resolved_path.as_deref(),
        Some(shell.to_str().unwrap()),
        "$SHELL expands to the user's shell"
    );

    let project = c.add_project(&d.project_dir("proj"));
    let session = c.create_session(&project, Some("/bin/sh"));
    c.attach(&session.id, 120, 30);
    c.send_pty(&session.id, b"echo login=$LOTSE_FROM_LOGIN p=${PATH%%:*}\n");
    assert!(
        c.wait_output(&session.id, "login=yes"),
        "{:?}",
        c.output_text(&session.id)
    );
    assert!(
        c.output_text(&session.id)
            .contains(&format!("p={}", tools.display()))
    );
}

#[test]
fn default_terminal_command_launches_the_login_shell() {
    let d = Daemon::start(); // SHELL=/bin/sh in the daemon's environment
    let mut c = d.client();
    let project = c.add_project(&d.project_dir("proj"));
    let Response::Session(s) = c.request(Request::CreateSession(lotse_proto::NewSession {
        project_id: project.id.clone(),
        agent: AgentKind::Terminal,
        command: None,
        title: None,
        worktree: None,
        first_prompt: None,
    })) else {
        panic!()
    };
    assert_eq!(s.command, "$SHELL -l");
    c.attach(&s.id, 80, 24);
    c.send_pty(
        &s.id,
        b"echo shell=$0 login=$(shopt -q login_shell && echo yes || echo no)\n",
    );
    assert!(
        c.wait_output(&s.id, "login=yes"),
        "{:?}",
        c.output_text(&s.id)
    );
}
