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
fn integration_install_status_uninstall_round_trip() {
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

    assert_eq!(say(&integration("status")).trim(), "not installed");
    let out = integration("install");
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(say(&out).contains("hooks installed"));
    assert_eq!(say(&integration("status")).trim(), "installed");
    // The installed command points at the binary that ran the install.
    let installed = std::fs::read_to_string(&settings).unwrap();
    let exe = std::fs::canonicalize(env!("CARGO_BIN_EXE_lotse-hook")).unwrap();
    assert!(installed.contains(exe.to_str().unwrap()), "{installed}");
    assert!(say(&integration("install")).contains("already installed"));

    assert!(say(&integration("uninstall")).contains("restored from backup"));
    assert_eq!(std::fs::read_to_string(&settings).unwrap(), original);

    // Bad usage fails (the only non-zero exits; hook mode never fails).
    let mut cmd = hook(home.path(), data.path());
    cmd.args(["integration", "claude", "explode"]);
    assert!(!run_with_stdin(cmd, b"").status.success());
}
