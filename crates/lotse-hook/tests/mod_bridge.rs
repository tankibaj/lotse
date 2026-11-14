//! `lotse-hook mod-bridge` / `mod-status` against a fake unix-socket daemon, the version gate, and the
//! mod files the injected Claude plugin carries. No real daemon, no real Claude.

use lotse_hook::claude_plugin;
use lotse_hook::forward::Config;
use lotse_hook::mod_bridge::{
    MIN_CLAUDE_VERSION, MOD_FILE, MOD_MODULE_PATH, MOD_SOURCE, mod_supported, run_bridge, run_send,
    status_payload,
};
use lotse_proto::{
    AgentKind, ErrorCode, Event, Frame, Hello, HelloOk, Message, ModCommand, PROTO_VERSION,
    ProtoError, Request, RequestEnvelope, Response, ResponseEnvelope, SessionId, read_frame,
    write_frame,
};
use serde_json::{Value, json};
use std::io::{Cursor, Write};
use std::os::unix::net::UnixListener;
use std::sync::mpsc::{self, Receiver};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::Duration;

/// What the fake daemon answers to `mod_attach`, and what it pushes afterwards.
struct Script {
    attach: Result<(), ErrorCode>,
    push: Vec<Event>,
    /// Requests to read (after the attach) before closing the connection.
    expect_requests: usize,
}

struct Fake {
    dir: tempfile::TempDir,
    hello: Receiver<Hello>,
    requests: Receiver<RequestEnvelope>,
    handle: JoinHandle<()>,
}

fn fake_daemon(token: &str, script: Script) -> Fake {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("token"), format!("{token}\n")).unwrap();
    let listener = UnixListener::bind(dir.path().join("lotsed.sock")).unwrap();
    let (hello_tx, hello) = mpsc::channel();
    let (req_tx, requests) = mpsc::channel();
    let handle = std::thread::spawn(move || {
        let (mut conn, _) = listener.accept().unwrap();
        let Some(Frame::Control(Message::Hello(h))) = read_frame(&mut conn).unwrap() else {
            panic!("expected hello first");
        };
        hello_tx.send(h).unwrap();
        let Some(Frame::Control(Message::Request(attach))) = read_frame(&mut conn).unwrap() else {
            panic!("expected the first request after hello");
        };
        req_tx.send(attach.clone()).unwrap();
        let send = |conn: &mut std::os::unix::net::UnixStream, m| {
            write_frame(conn, &Frame::Control(m)).unwrap();
        };
        send(
            &mut conn,
            Message::HelloOk(HelloOk {
                proto_version: PROTO_VERSION,
                daemon_version: "test".into(),
            }),
        );
        let response = match script.attach {
            Ok(()) => Response::Ok,
            Err(code) => Response::Error(ProtoError {
                code,
                message: "no".into(),
            }),
        };
        send(
            &mut conn,
            Message::Response(ResponseEnvelope {
                req_id: attach.req_id,
                response,
            }),
        );
        if script.attach.is_err() {
            return;
        }
        for event in script.push {
            send(&mut conn, Message::Event(event));
        }
        for _ in 0..script.expect_requests {
            let Some(Frame::Control(Message::Request(req))) = read_frame(&mut conn).unwrap() else {
                break;
            };
            req_tx.send(req.clone()).unwrap();
            send(
                &mut conn,
                Message::Response(ResponseEnvelope {
                    req_id: req.req_id,
                    response: Response::Ok,
                }),
            );
        }
    });
    Fake {
        dir,
        hello,
        requests,
        handle,
    }
}

fn cfg(d: &Fake, session: Option<&str>) -> Config {
    let mut cfg = Config::in_data_dir(
        d.dir.path(),
        AgentKind::ClaudeCode,
        session.map(str::to_owned),
    );
    cfg.deadline = Duration::from_secs(5);
    cfg
}

#[derive(Clone, Default)]
struct Sink(Arc<Mutex<Vec<u8>>>);

impl Write for Sink {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.0.lock().unwrap().extend_from_slice(buf);
        Ok(buf.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

impl Sink {
    fn lines(&self) -> Vec<Value> {
        String::from_utf8(self.0.lock().unwrap().clone())
            .unwrap()
            .lines()
            .map(|l| serde_json::from_str(l).unwrap())
            .collect()
    }
}

fn mod_command(session: &str, command: ModCommand) -> Event {
    Event::ModCommand {
        session_id: SessionId::new(session),
        command,
    }
}

#[test]
fn the_bridge_attaches_with_the_token_and_relays_only_its_sessions_commands() {
    let d = fake_daemon(
        "s3cret",
        Script {
            attach: Ok(()),
            push: vec![
                mod_command("other", ModCommand::Submit { text: "no".into() }),
                mod_command("a1", ModCommand::Ping),
                mod_command("a1", ModCommand::Submit { text: "hi".into() }),
            ],
            expect_requests: 0,
        },
    );
    let out = Sink::default();
    run_bridge(&cfg(&d, Some("a1")), std::io::empty(), out.clone()).unwrap();

    let hello = d.hello.recv().unwrap();
    assert_eq!(hello.token, "s3cret");
    assert_eq!(hello.client, "mod");
    assert_eq!(
        d.requests.recv().unwrap().request,
        Request::ModAttach {
            session_id: SessionId::new("a1")
        }
    );
    assert_eq!(
        out.lines(),
        [
            json!({"bridge": "attached"}),
            json!({"cmd": "ping"}),
            json!({"cmd": "submit", "text": "hi"}),
        ]
    );
    d.handle.join().unwrap();
}

#[test]
fn an_answer_is_relayed_with_its_decision_and_the_hold_lines_reach_the_daemon_as_status() {
    let d = fake_daemon(
        "s3cret",
        Script {
            attach: Ok(()),
            push: vec![mod_command(
                "a1",
                ModCommand::Answer {
                    prompt_id: Some("p-1".into()),
                    decision: lotse_proto::PermDecision::Deny,
                },
            )],
            expect_requests: 3,
        },
    );
    let out = Sink::default();
    let stdin = Cursor::new(
        "{\"t\":\"perm_hold\",\"tool\":\"Bash\"}\n{\"t\":\"perm_release\"}\n{\"t\":\"submit_result\",\"ok\":false}\n",
    );
    run_bridge(&cfg(&d, Some("a1")), stdin, out.clone()).unwrap();
    assert_eq!(
        out.lines(),
        [
            json!({"bridge": "attached"}),
            json!({"cmd": "answer", "prompt_id": "p-1", "decision": "deny"}),
        ]
    );
    d.hello.recv().unwrap();
    d.requests.recv().unwrap(); // the attach
    let kinds: Vec<String> = (0..3)
        .map(|_| match d.requests.recv().unwrap().request {
            Request::HookEvent { payload, .. } => {
                assert_eq!(payload["kind"], "lotse_mod");
                payload["t"].as_str().unwrap().to_owned()
            }
            other => panic!("{other:?}"),
        })
        .collect();
    assert_eq!(kinds, ["perm_hold", "perm_release", "submit_result"]);
    d.handle.join().unwrap();
}

#[test]
fn status_lines_on_stdin_reach_the_daemon_as_this_sessions_lotse_mod_events() {
    let d = fake_daemon(
        "t",
        Script {
            attach: Ok(()),
            push: vec![],
            expect_requests: 2,
        },
    );
    let stdin = Cursor::new(
        concat!(
            r#"{"t":"tool","agent_id":"x1","tool":"Bash","summary":"ls"}"#,
            "\n",
            "not json\n",
            r#"[1,2]"#,
            "\n",
            r#"{"no_t":true}"#,
            "\n",
            r#"{"t":"hello","hook_event_name":"Stop","session_id":"evil","lotse_source":"x"}"#,
            "\n",
        )
        .as_bytes()
        .to_vec(),
    );
    let out = Sink::default();
    run_bridge(&cfg(&d, Some("a1")), stdin, out.clone()).unwrap();
    d.handle.join().unwrap();

    let _attach = d.requests.recv().unwrap();
    let mut payloads = vec![];
    while let Ok(req) = d.requests.try_recv() {
        let Request::HookEvent {
            agent,
            session_id,
            payload,
        } = req.request
        else {
            panic!("expected hook events");
        };
        assert_eq!(agent, AgentKind::ClaudeCode);
        assert_eq!(session_id, Some(SessionId::new("a1")), "never the line's");
        payloads.push(payload);
    }
    assert_eq!(
        payloads,
        [
            json!({"kind": "lotse_mod", "lotse_source": "mod", "t": "tool", "agent_id": "x1",
                   "tool": "Bash", "summary": "ls"}),
            // a mod line can neither pose as a hook event nor name another session
            json!({"kind": "lotse_mod", "lotse_source": "mod", "t": "hello"}),
        ]
    );
}

#[test]
fn a_refused_attach_is_an_error_and_never_prints_attached() {
    let d = fake_daemon(
        "t",
        Script {
            attach: Err(ErrorCode::Unavailable),
            push: vec![],
            expect_requests: 0,
        },
    );
    let out = Sink::default();
    let err = run_bridge(&cfg(&d, Some("a1")), std::io::empty(), out.clone()).unwrap_err();
    assert!(err.to_string().contains("refused"), "{err:#}");
    assert!(out.lines().is_empty());
    d.handle.join().unwrap();
}

#[test]
fn outside_lotse_the_bridge_connects_to_nothing() {
    let dir = tempfile::tempdir().unwrap(); // no socket, no token: a connect would fail
    let cfg = Config::in_data_dir(dir.path(), AgentKind::ClaudeCode, None);
    let out = Sink::default();
    run_bridge(&cfg, std::io::empty(), out.clone()).unwrap();
    assert!(out.lines().is_empty());
    assert_eq!(run_send(&cfg, &b"{\"t\":\"x\"}\n"[..]).unwrap(), 0);
}

#[test]
fn mod_status_sends_every_valid_line_over_one_connection() {
    let d = fake_daemon(
        "t",
        Script {
            attach: Ok(()), // answers the first request, whatever it is
            push: vec![],
            expect_requests: 2,
        },
    );
    let input =
        b"{\"t\":\"heartbeat\",\"context_pct\":12}\nnope\n{\"t\":\"tool\"}\n{\"t\":\"perm\"}\n";
    let sent = run_send(&cfg(&d, Some("a1")), &input[..]).unwrap();
    assert_eq!(sent, 3);
    d.handle.join().unwrap();
    let kinds: Vec<Value> = d
        .requests
        .try_iter()
        .map(|r| {
            let Request::HookEvent { payload, .. } = r.request else {
                panic!("expected a hook event, got {:?}", r.request);
            };
            payload["t"].clone()
        })
        .collect();
    assert_eq!(kinds, [json!("heartbeat"), json!("tool"), json!("perm")]);
}

#[test]
fn status_payload_rules() {
    assert!(status_payload("").is_none());
    assert!(status_payload("{\"t\":\"x\"").is_none());
    assert!(status_payload("{\"t\":1}").is_none(), "t must be a string");
    let long = format!("{{\"t\":\"x\",\"pad\":\"{}\"}}", "a".repeat(70_000));
    assert!(status_payload(&long).is_none(), "oversized");
    assert_eq!(
        status_payload(r#"{"t":"x","n":1}"#).unwrap(),
        json!({"kind": "lotse_mod", "lotse_source": "mod", "t": "x", "n": 1})
    );
}

#[test]
fn the_version_gate_wants_claude_2_1_311_or_newer() {
    assert_eq!(MIN_CLAUDE_VERSION, "2.1.311");
    for ok in [
        "2.1.311 (Claude Code)",
        "2.1.311",
        "2.1.400",
        "2.2.0",
        "3.0.0 (Claude Code)",
        "10.0.0",
        "claude 2.1.312\n",
        "2.1.311-dev.20260920.t1.sha1",
    ] {
        assert!(mod_supported(ok), "{ok}");
    }
    for bad in [
        "2.1.310 (Claude Code)",
        "2.0.999",
        "1.9.9999",
        "",
        "Claude Code",
        "garbage",
        "2.1",
    ] {
        assert!(!mod_supported(bad), "{bad}");
    }
}

fn parse(files: &[(&str, String)], name: &str) -> Value {
    let (_, text) = files.iter().find(|(f, _)| *f == name).unwrap();
    serde_json::from_str(text).unwrap()
}

#[test]
fn the_plugin_carries_the_mod_files_and_names_the_module() {
    let files = claude_plugin::files().unwrap();
    let hooks = parse(&files, "hooks/hooks.json");
    assert_eq!(hooks["modules"], json!([MOD_MODULE_PATH]));
    assert!(hooks["hooks"].as_object().is_some_and(|h| !h.is_empty()));
    let (_, source) = files.iter().find(|(f, _)| *f == MOD_FILE).unwrap();
    assert_eq!(source, MOD_SOURCE);
    // the module path resolves, from hooks/hooks.json, to the written module file
    assert_eq!(
        std::path::Path::new("hooks").join(MOD_MODULE_PATH),
        std::path::Path::new("hooks/../mod/register.js")
    );
    assert_eq!(MOD_FILE, "mod/register.js");
}

#[test]
fn without_the_mod_the_plugin_is_hooks_only() {
    let files = claude_plugin::files_for(false).unwrap();
    let hooks = parse(&files, "hooks/hooks.json");
    assert!(hooks.get("modules").is_none());
    assert!(files.iter().all(|(f, _)| *f != MOD_FILE));
}

#[test]
fn ensure_writes_the_mod_next_to_the_hooks() {
    let tmp = tempfile::tempdir().unwrap();
    let root = claude_plugin::ensure(tmp.path()).unwrap();
    assert_eq!(
        std::fs::read_to_string(root.join(MOD_FILE)).unwrap(),
        MOD_SOURCE
    );
    let off = tempfile::tempdir().unwrap();
    let root = claude_plugin::ensure_for(off.path(), false).unwrap();
    assert!(!root.join(MOD_FILE).exists());
}

#[test]
fn the_module_source_follows_the_loader_rules() {
    assert!(
        !MOD_SOURCE.contains("import("),
        "a module holding import() does not load"
    );
    assert!(!MOD_SOURCE.contains("require("));
    // `$.env.get` takes a literal name
    for (at, _) in MOD_SOURCE.match_indices("$.env.get(") {
        let rest = &MOD_SOURCE[at + "$.env.get(".len()..];
        assert!(
            rest.starts_with('\'') || rest.starts_with('"'),
            "{}",
            &rest[..30]
        );
    }
    assert!(MOD_SOURCE.contains("export const register"));
    // the bridge it starts is ours, by argument vector (no shell)
    assert!(MOD_SOURCE.contains("'mod-bridge'") && MOD_SOURCE.contains("'mod-status'"));
    // never a token, a socket or a free command from outside
    for forbidden in ["LOTSE_TOKEN", "LOTSE_SOCKET", "token_file"] {
        assert!(!MOD_SOURCE.contains(forbidden), "{forbidden}");
    }
}

fn binary(d: &Fake, session: &str) -> std::process::Command {
    let mut cmd = std::process::Command::new(env!("CARGO_BIN_EXE_lotse-hook"));
    cmd.env_clear()
        .env("LOTSE_SESSION_ID", session)
        .env("LOTSE_SOCKET", d.dir.path().join("lotsed.sock"))
        .env("LOTSE_TOKEN_FILE", d.dir.path().join("token"));
    cmd
}

#[test]
fn the_binary_runs_mod_status_silently_and_exits_zero() {
    let d = fake_daemon(
        "t",
        Script {
            attach: Ok(()),
            push: vec![],
            expect_requests: 0,
        },
    );
    let mut child = binary(&d, "a1")
        .arg("mod-status")
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(b"{\"t\":\"heartbeat\"}\n")
        .unwrap();
    let out = child.wait_with_output().unwrap();
    assert!(out.status.success());
    assert!(out.stdout.is_empty() && out.stderr.is_empty());
    let req = d.requests.recv_timeout(Duration::from_secs(5)).unwrap();
    let Request::HookEvent { payload, .. } = req.request else {
        panic!("expected a hook event");
    };
    assert_eq!(payload["t"], "heartbeat");
}

#[test]
fn the_binary_runs_mod_bridge_and_prints_the_daemons_commands() {
    let d = fake_daemon(
        "t",
        Script {
            attach: Ok(()),
            push: vec![mod_command("a1", ModCommand::Ping)],
            expect_requests: 0,
        },
    );
    let out = binary(&d, "a1")
        .arg("mod-bridge")
        .stdin(std::process::Stdio::null())
        .output()
        .unwrap();
    assert!(out.status.success());
    assert_eq!(
        String::from_utf8(out.stdout).unwrap(),
        "{\"bridge\":\"attached\"}\n{\"cmd\":\"ping\"}\n"
    );
    d.handle.join().unwrap();
}

#[test]
fn the_binary_exits_non_zero_when_the_attach_is_refused() {
    let d = fake_daemon(
        "t",
        Script {
            attach: Err(ErrorCode::Unavailable),
            push: vec![],
            expect_requests: 0,
        },
    );
    let out = binary(&d, "a1")
        .arg("mod-bridge")
        .stdin(std::process::Stdio::null())
        .output()
        .unwrap();
    assert!(!out.status.success());
    assert!(out.stdout.is_empty());
}
