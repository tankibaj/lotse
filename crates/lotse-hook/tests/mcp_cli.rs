//! `lotse-hook mcp` as the agent runs it: JSON-RPC lines on stdin and stdout, a fake `lotsed` on a
//! temp Unix socket. The child gets a temp HOME and data dir and nothing from the caller's
//! environment; the token travels in a file, never in its environment or arguments.

use lotse_proto::{
    Frame, GitInfo, HelloOk, Message, PROTO_VERSION, Request, Response, ResponseEnvelope, Session,
    SessionId, read_frame, write_frame,
};
use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixListener;
use std::process::{Command, Stdio};
use std::sync::mpsc;

fn kid() -> Session {
    Session {
        id: SessionId::new("s-kid"),
        project_id: lotse_proto::ProjectId::new("p1"),
        title: "Fix".into(),
        agent: lotse_proto::AgentKind::ClaudeCode,
        command: "claude".into(),
        status: lotse_proto::Status::Done,
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
        parent_session: Some(SessionId::new("s-me")),
        todo: None,
    }
}

#[test]
fn the_server_speaks_mcp_on_stdio_and_reaches_the_daemon_as_mcp_with_the_token_from_its_file() {
    let home = tempfile::tempdir().unwrap();
    let data = tempfile::tempdir().unwrap();
    std::fs::write(data.path().join("token"), "tok-from-file\n").unwrap();
    let listener = UnixListener::bind(data.path().join("lotsed.sock")).unwrap();
    let (seen_tx, seen) = mpsc::channel();
    std::thread::spawn(move || {
        let (mut conn, _) = listener.accept().unwrap();
        let Some(Frame::Control(Message::Hello(hello))) = read_frame(&mut conn).unwrap() else {
            panic!("hello expected");
        };
        seen_tx.send((hello.token, hello.client)).unwrap();
        let ok = Message::HelloOk(HelloOk {
            proto_version: PROTO_VERSION,
            daemon_version: "t".into(),
        });
        write_frame(&mut conn, &Frame::Control(ok)).unwrap();
        while let Ok(Some(Frame::Control(Message::Request(req)))) = read_frame(&mut conn) {
            let response = match req.request {
                Request::ListSessions { .. } => Response::Sessions(vec![kid()]),
                _ => Response::Ok,
            };
            let m = Message::Response(ResponseEnvelope {
                req_id: req.req_id,
                response,
            });
            write_frame(&mut conn, &Frame::Control(m)).unwrap();
        }
    });

    let mut child = Command::new(env!("CARGO_BIN_EXE_lotse-hook"))
        .arg("mcp")
        .env_clear()
        .env("HOME", home.path())
        .env("LOTSE_SESSION_ID", "s-me")
        .env("LOTSE_PROJECT_ID", "p1")
        .env("LOTSE_SOCKET", data.path().join("lotsed.sock"))
        .env("LOTSE_TOKEN_FILE", data.path().join("token"))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let mut stdin = child.stdin.take().unwrap();
    let mut stdout = BufReader::new(child.stdout.take().unwrap());
    let mut ask = |line: &str| -> serde_json::Value {
        writeln!(stdin, "{line}").unwrap();
        let mut answer = String::new();
        stdout.read_line(&mut answer).unwrap();
        serde_json::from_str(&answer).unwrap()
    };

    let v = ask(
        r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-06-18"}}"#,
    );
    assert_eq!(v["result"]["serverInfo"]["name"], "lotse");
    let v = ask(r#"{"jsonrpc":"2.0","id":2,"method":"tools/list"}"#);
    assert_eq!(v["result"]["tools"].as_array().unwrap().len(), 6);
    let v = ask(
        r#"{"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"list_sessions","arguments":{}}}"#,
    );
    let text = v["result"]["content"][0]["text"].as_str().unwrap();
    assert!(text.contains("s-kid"), "{v}");
    assert_ne!(v["result"]["isError"], true);

    let (token, client) = seen.recv().unwrap();
    assert_eq!(token, "tok-from-file");
    assert_eq!(client, "mcp:s-me");

    drop(stdin);
    let out = child.wait_with_output().unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(
        !String::from_utf8_lossy(&out.stderr).contains("tok-from-file"),
        "the token is never printed"
    );
}

#[test]
fn without_a_daemon_the_server_still_answers_and_the_tool_fails_cleanly() {
    let home = tempfile::tempdir().unwrap();
    let data = tempfile::tempdir().unwrap();
    let mut child = Command::new(env!("CARGO_BIN_EXE_lotse-hook"))
        .arg("mcp")
        .env_clear()
        .env("HOME", home.path())
        .env("LOTSE_SESSION_ID", "s-me")
        .env("LOTSE_DATA_DIR", data.path())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let mut stdin = child.stdin.take().unwrap();
    writeln!(
        stdin,
        r#"{{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{{"name":"list_sessions","arguments":{{}}}}}}"#
    )
    .unwrap();
    drop(stdin);
    let out = child.wait_with_output().unwrap();
    assert!(out.status.success());
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(v["result"]["isError"], true);
}
