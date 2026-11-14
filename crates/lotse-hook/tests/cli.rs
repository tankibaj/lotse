//! End-to-end tests of the `lotse-hook` binary. Every child process gets a temp `HOME`, data dir and
//! config dir, and no `CLAUDE_CONFIG_DIR`/`LOTSE_*` from the caller, so nothing real is touched.

use lotse_proto::{
    AgentKind, Frame, HelloOk, Message, PROTO_VERSION, Request, Response, ResponseEnvelope,
    SessionId, read_frame, write_frame,
};
use std::os::unix::net::UnixListener;
use std::path::Path;
use std::process::{Command, Output, Stdio};
use std::time::{Duration, Instant};

fn hook(home: &Path, data: &Path) -> Command {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_lotse-hook"));
    cmd.env_clear()
        .env("HOME", home)
        .env("LOTSE_DATA_DIR", data)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    cmd
}

fn run_with_stdin(mut cmd: Command, input: &[u8]) -> Output {
    use std::io::Write;
    let mut child = cmd.spawn().unwrap();
    child.stdin.take().unwrap().write_all(input).unwrap();
    child.wait_with_output().unwrap()
}

#[test]
fn delivers_a_hook_event_silently_and_exits_zero() {
    let home = tempfile::tempdir().unwrap();
    let data = tempfile::tempdir().unwrap();
    std::fs::write(data.path().join("token"), "tok\n").unwrap();
    let listener = UnixListener::bind(data.path().join("lotsed.sock")).unwrap();
    let daemon = std::thread::spawn(move || {
        let (mut conn, _) = listener.accept().unwrap();
        let Some(Frame::Control(Message::Hello(hello))) = read_frame(&mut conn).unwrap() else {
            panic!("hello expected");
        };
        let Some(Frame::Control(Message::Request(req))) = read_frame(&mut conn).unwrap() else {
            panic!("request expected");
        };
        let reply = |conn: &mut std::os::unix::net::UnixStream, m| {
            write_frame(conn, &Frame::Control(m)).unwrap()
        };
        reply(
            &mut conn,
            Message::HelloOk(HelloOk {
                proto_version: PROTO_VERSION,
                daemon_version: "t".into(),
            }),
        );
        reply(
            &mut conn,
            Message::Response(ResponseEnvelope {
                req_id: req.req_id,
                response: Response::Ok,
            }),
        );
        (hello, req)
    });

    let mut cmd = hook(home.path(), data.path());
    cmd.env("LOTSE_SESSION_ID", "a3").arg("claude");
    let started = Instant::now();
    let out = run_with_stdin(cmd, br#"{"hook_event_name":"Stop","session_id":"c0ffee"}"#);
    assert!(out.status.success());
    assert!(
        out.stdout.is_empty(),
        "hook stdout reaches the agent: {:?}",
        out.stdout
    );
    assert!(out.stderr.is_empty());
    assert!(started.elapsed() < Duration::from_secs(10));

    let (hello, req) = daemon.join().unwrap();
    assert_eq!(hello.token, "tok");
    assert_eq!(
        req.request,
        Request::HookEvent {
            agent: AgentKind::ClaudeCode,
            session_id: Some(SessionId::new("a3")),
            payload: serde_json::json!({"hook_event_name": "Stop", "session_id": "c0ffee"}),
        }
    );
}

#[test]
fn exits_zero_and_silent_when_the_daemon_is_down_or_not_in_lotse() {
    let home = tempfile::tempdir().unwrap();
    let data = tempfile::tempdir().unwrap();
    let payload = br#"{"hook_event_name":"PreToolUse"}"#;

    // In a Lotse session, but no socket and no token.
    let mut cmd = hook(home.path(), data.path());
    cmd.env("LOTSE_SESSION_ID", "a1");
    let started = Instant::now();
    let out = run_with_stdin(cmd, payload);
    assert!(out.status.success());
    assert!(out.stdout.is_empty() && out.stderr.is_empty());
    assert!(
        started.elapsed() < Duration::from_secs(5),
        "{:?}",
        started.elapsed()
    );

    // Outside Lotse (no session id), unknown agent, garbage input: still zero, still silent.
    for (args, env_session, input) in [
        (vec![], false, &payload[..]),
        (vec!["nope"], true, &payload[..]),
        (vec!["claude"], true, &b"not json"[..]),
        (vec!["claude"], true, &b""[..]),
    ] {
        let mut cmd = hook(home.path(), data.path());
        cmd.args(args);
        if env_session {
            cmd.env("LOTSE_SESSION_ID", "a1");
        }
        let out = run_with_stdin(cmd, input);
        assert!(out.status.success());
        assert!(out.stdout.is_empty() && out.stderr.is_empty());
    }
}

#[test]
fn debug_env_explains_a_failed_delivery_on_stderr_only() {
    let home = tempfile::tempdir().unwrap();
    let data = tempfile::tempdir().unwrap();
    let mut cmd = hook(home.path(), data.path());
    cmd.env("LOTSE_SESSION_ID", "a1")
        .env("LOTSE_HOOK_DEBUG", "1");
    let out = run_with_stdin(cmd, b"{}");
    assert!(out.status.success());
    assert!(out.stdout.is_empty());
    assert!(String::from_utf8_lossy(&out.stderr).contains("lotse-hook:"));
}

#[test]
fn the_cli_never_installs_global_hooks_but_removes_an_old_install() {
    let home = tempfile::tempdir().unwrap();
    let data = tempfile::tempdir().unwrap();
    let cfg = tempfile::tempdir().unwrap();
    let settings = cfg.path().join("settings.json");
    let original = "{\n  \"model\": \"opus\"\n}\n";
    std::fs::write(&settings, original).unwrap();

    let integration = |action: &str| -> Output {
        let mut cmd = hook(home.path(), data.path());
        cmd.args(["integration", "claude", action, "--config-dir"])
            .arg(cfg.path());
        run_with_stdin(cmd, b"")
    };
    let say = |out: &Output| String::from_utf8_lossy(&out.stdout).into_owned();

    // `install` is gone: it fails and leaves the settings alone
    assert!(!integration("install").status.success());
    assert_eq!(std::fs::read_to_string(&settings).unwrap(), original);
    assert_eq!(say(&integration("status")).trim(), "not installed");

    // an install made by an older build is reported and removed
    let exe = std::fs::canonicalize(env!("CARGO_BIN_EXE_lotse-hook")).unwrap();
    lotse_hook::claude_settings::install(cfg.path(), &exe).unwrap();
    assert_eq!(say(&integration("status")).trim(), "installed");
    assert!(say(&integration("uninstall")).contains("restored from backup"));
    assert_eq!(std::fs::read_to_string(&settings).unwrap(), original);

    // Bad usage fails (the only non-zero exits; hook mode never fails).
    let mut cmd = hook(home.path(), data.path());
    cmd.args(["integration", "claude", "explode"]);
    assert!(!run_with_stdin(cmd, b"").status.success());
}

#[test]
fn the_cli_never_installs_the_opencode_plugin_but_removes_an_old_one() {
    let home = tempfile::tempdir().unwrap();
    let data = tempfile::tempdir().unwrap();
    let xdg = tempfile::tempdir().unwrap();
    let config = xdg.path().join("opencode");
    let plugins = config.join("plugins");
    let integration = |action: &str| -> Output {
        let mut cmd = hook(home.path(), data.path());
        cmd.env("XDG_CONFIG_HOME", xdg.path())
            .args(["integration", "opencode", action]);
        run_with_stdin(cmd, b"")
    };
    let say = |out: &Output| String::from_utf8_lossy(&out.stdout).trim().to_owned();

    assert!(!integration("install").status.success());
    assert!(!plugins.join("lotse-status.js").exists());
    assert_eq!(say(&integration("status")), "not installed");

    // an install made by an older build is reported and removed
    lotse_hook::opencode_plugin::install(&config).unwrap();
    assert_eq!(say(&integration("status")), "installed");
    assert_eq!(say(&integration("uninstall")), "plugin removed");
    assert!(!plugins.join("lotse-status.js").exists());

    // a file of the same name that is not ours: never removed
    let foreign = "// somebody else's plugin\n";
    std::fs::write(plugins.join("lotse-status.js"), foreign).unwrap();
    let _ = integration("uninstall");
    assert_eq!(
        std::fs::read_to_string(plugins.join("lotse-status.js")).unwrap(),
        foreign
    );
}

#[test]
fn the_injected_flag_tags_the_event_for_the_daemon() {
    let home = tempfile::tempdir().unwrap();
    let data = tempfile::tempdir().unwrap();
    std::fs::write(data.path().join("token"), "tok\n").unwrap();
    let listener = UnixListener::bind(data.path().join("lotsed.sock")).unwrap();
    let daemon = std::thread::spawn(move || {
        let (mut conn, _) = listener.accept().unwrap();
        let _hello = read_frame(&mut conn).unwrap();
        let Some(Frame::Control(Message::Request(req))) = read_frame(&mut conn).unwrap() else {
            panic!("request expected");
        };
        let reply = Message::Response(ResponseEnvelope {
            req_id: req.req_id,
            response: Response::Ok,
        });
        write_frame(&mut conn, &Frame::Control(reply)).unwrap();
        req
    });
    let mut cmd = hook(home.path(), data.path());
    cmd.env("LOTSE_SESSION_ID", "a3")
        .args(["claude", "--injected"]);
    let out = run_with_stdin(cmd, br#"{"hook_event_name":"Stop"}"#);
    assert!(out.status.success() && out.stdout.is_empty());
    let Request::HookEvent { payload, .. } = daemon.join().unwrap().request else {
        panic!("hook event expected");
    };
    assert_eq!(payload["lotse_source"], "injected");
}

#[test]
fn the_codex_hook_command_of_the_mirror_delivers_a_tagged_codex_event_through_a_shell() {
    let home = tempfile::tempdir().unwrap();
    let data = tempfile::tempdir().unwrap();
    // the daemon exports the binary's path in LOTSE_HOOK_BIN; one with a space must survive the shell
    let bin_dir = home.path().join("Application Support");
    std::fs::create_dir_all(&bin_dir).unwrap();
    let bin = bin_dir.join("lotse-hook");
    std::os::unix::fs::symlink(env!("CARGO_BIN_EXE_lotse-hook"), &bin).unwrap();
    std::fs::write(data.path().join("token"), "tok\n").unwrap();
    let listener = UnixListener::bind(data.path().join("lotsed.sock")).unwrap();
    let daemon = std::thread::spawn(move || {
        let (mut conn, _) = listener.accept().unwrap();
        let _hello = read_frame(&mut conn).unwrap();
        let Some(Frame::Control(Message::Request(req))) = read_frame(&mut conn).unwrap() else {
            panic!("request expected");
        };
        let reply = Message::Response(ResponseEnvelope {
            req_id: req.req_id,
            response: Response::Ok,
        });
        write_frame(&mut conn, &Frame::Control(reply)).unwrap();
        req
    });
    let mut cmd = Command::new("/bin/sh");
    cmd.env_clear()
        .env("HOME", home.path())
        .env("LOTSE_DATA_DIR", data.path())
        .env("LOTSE_SESSION_ID", "c7")
        .env("LOTSE_HOOK_BIN", &bin)
        .args(["-c", lotse_hook::codex_home::HOOK_COMMAND])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let out = run_with_stdin(
        cmd,
        br#"{"hook_event_name":"SubagentStart","agent_id":"a1","agent_type":"worker","prompt":"private"}"#,
    );
    assert!(out.status.success() && out.stdout.is_empty() && out.stderr.is_empty());
    assert_eq!(
        daemon.join().unwrap().request,
        Request::HookEvent {
            agent: AgentKind::Codex,
            session_id: Some(SessionId::new("c7")),
            payload: serde_json::json!({
                "hook_event_name": "SubagentStart",
                "agent_id": "a1",
                "agent_type": "worker",
                "lotse_source": "injected",
            }),
        }
    );
}
