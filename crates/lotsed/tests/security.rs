//! Socket and file modes, token handshake, and the no-TCP rule.

mod common;

use common::*;
use lotse_proto::{
    ErrorCode, Frame, Message, PROTO_VERSION, Request, RequestEnvelope, Response, read_frame,
    write_frame,
};
use std::io::Write;
use std::os::unix::fs::{MetadataExt, PermissionsExt};
use std::os::unix::net::UnixStream;
use std::path::Path;
use std::process::Command;

fn mode(p: &Path) -> u32 {
    std::fs::metadata(p).unwrap().permissions().mode() & 0o777
}

#[test]
fn socket_token_pidfile_database_and_directory_modes() {
    let d = Daemon::start();
    let mut c = d.client();
    c.add_project(&d.project_dir("proj")); // makes sure the database was written

    assert_eq!(mode(&d.data_dir), 0o700, "data dir");
    assert_eq!(mode(&d.socket()), 0o600, "socket");
    assert_eq!(mode(&d.token_path()), 0o600, "token");
    assert_eq!(mode(&d.data_dir.join("lotsed.pid")), 0o600, "pidfile");
    assert_eq!(mode(&d.data_dir.join("lotse.db")), 0o600, "database");
    let me = nix::unistd::getuid().as_raw();
    for p in [d.data_dir.clone(), d.socket(), d.token_path()] {
        assert_eq!(std::fs::metadata(p).unwrap().uid(), me);
    }
    // 256-bit token, hex
    let t = d.token();
    assert_eq!(t.len(), 64);
    assert!(t.bytes().all(|b| b.is_ascii_hexdigit()));
}

#[test]
fn a_loose_data_dir_is_tightened_on_start() {
    let d = Daemon::start();
    std::fs::set_permissions(&d.data_dir, std::fs::Permissions::from_mode(0o755)).unwrap();
    std::fs::set_permissions(d.token_path(), std::fs::Permissions::from_mode(0o644)).unwrap();
    let d = d.restart();
    assert_eq!(mode(&d.data_dir), 0o700);
    assert_eq!(mode(&d.token_path()), 0o600);
}

fn rejected(token: &str, version: u32) -> ErrorCode {
    let d = Daemon::start();
    let mut s = UnixStream::connect(d.socket()).unwrap();
    hello(&mut s, token, version);
    let code = match read_frame(&mut s).unwrap() {
        Some(Frame::Control(Message::Response(r))) => {
            assert_eq!(r.req_id, 0, "handshake errors use req_id 0");
            match r.response {
                Response::Error(e) => e.code,
                other => panic!("{other:?}"),
            }
        }
        other => panic!("{other:?}"),
    };
    assert!(
        read_frame(&mut s).unwrap().is_none(),
        "connection must be closed after a rejection"
    );
    code
}

#[test]
fn wrong_token_is_unauthorized_and_closed() {
    assert_eq!(
        rejected("0123456789abcdef0123456789abcdef", PROTO_VERSION),
        ErrorCode::Unauthorized
    );
    assert_eq!(rejected("", PROTO_VERSION), ErrorCode::Unauthorized);
}

#[test]
fn wrong_protocol_version_is_rejected() {
    let d = Daemon::start();
    let mut s = UnixStream::connect(d.socket()).unwrap();
    hello(&mut s, &d.token(), PROTO_VERSION + 1);
    match read_frame(&mut s).unwrap() {
        Some(Frame::Control(Message::Response(r))) => match r.response {
            Response::Error(e) => assert_eq!(e.code, ErrorCode::VersionMismatch),
            other => panic!("{other:?}"),
        },
        other => panic!("{other:?}"),
    }
    assert!(read_frame(&mut s).unwrap().is_none());
}

#[test]
fn requests_before_hello_get_nothing() {
    let d = Daemon::start();
    let mut s = UnixStream::connect(d.socket()).unwrap();
    write_frame(
        &mut s,
        &Frame::Control(Message::Request(RequestEnvelope {
            req_id: 1,
            request: Request::GetSnapshot,
        })),
    )
    .unwrap();
    assert!(
        matches!(read_frame(&mut s), Ok(None) | Err(_)),
        "no data may leak before the token"
    );
}

#[test]
fn a_connection_that_never_says_hello_is_closed_after_the_hello_timeout() {
    let d = Daemon::start();
    let mut s = UnixStream::connect(d.socket()).unwrap();
    s.set_read_timeout(Some(std::time::Duration::from_secs(12)))
        .unwrap();
    let started = std::time::Instant::now();
    assert!(matches!(read_frame(&mut s), Ok(None) | Err(_)));
    let waited = started.elapsed();
    assert!(
        waited >= std::time::Duration::from_secs(4),
        "closed after {waited:?}"
    );
    assert!(
        waited < std::time::Duration::from_secs(9),
        "closed after {waited:?}"
    );
}

#[test]
fn oversized_unauthenticated_frame_is_dropped() {
    let d = Daemon::start();
    let mut s = UnixStream::connect(d.socket()).unwrap();
    // announces 8 MiB before any token: the daemon must not buffer it
    let _ = s.write_all(&(8u32 * 1024 * 1024).to_be_bytes());
    let _ = s.write_all(&[0x01; 64]);
    assert!(matches!(read_frame(&mut s), Ok(None) | Err(_)));
    // the daemon is fine
    let mut c = d.client();
    assert!(matches!(c.request(Request::Ping), Response::Pong));
}

#[test]
fn garbage_after_a_good_hello_only_drops_that_client() {
    let d = Daemon::start();
    let mut good = d.client();
    let mut bad = d.client();
    // a frame with an unknown type byte
    let mut raw = 2u32.to_be_bytes().to_vec();
    raw.extend([0x7f, 0x00]);
    bad.write_raw(&raw);
    assert!(bad.wait_closed());
    assert!(matches!(good.request(Request::Ping), Response::Pong));
}

/// The Done-when check: `lsof -iTCP -a -p <pid>` is empty, with sessions and clients active.
#[test]
fn the_daemon_has_no_tcp_listener() {
    let d = Daemon::start();
    let mut c = d.client();
    let project = c.add_project(&d.project_dir("proj"));
    let session = c.create_session(&project, Some("/bin/sh"));
    c.attach(&session.id, 80, 24);

    let pid = d.pid().to_string();
    // sanity: lsof can see the daemon at all (otherwise an empty answer proves nothing)
    let all = Command::new("lsof")
        .args(["-nP", "-p", &pid])
        .output()
        .expect("lsof");
    let all = String::from_utf8_lossy(&all.stdout);
    assert!(
        all.contains("lotsed.sock"),
        "lsof does not see the daemon's socket:\n{all}"
    );

    for filter in ["-iTCP", "-iUDP", "-i"] {
        let out = Command::new("lsof")
            .args(["-nP", filter, "-a", "-p", &pid])
            .output()
            .expect("lsof");
        let text = String::from_utf8_lossy(&out.stdout);
        assert!(
            text.trim().is_empty(),
            "lsof {filter} -a -p {pid} found sockets:\n{text}"
        );
    }
}
