//! Detached start, pidfile, graceful shutdown and persistence across daemon restarts.

mod common;

use common::*;
use lotse_proto::{Request, Response};
use nix::sys::signal::{Signal, kill};
use nix::unistd::Pid;
use std::process::Command;
use std::time::Duration;

#[test]
fn shutdown_request_stops_the_daemon_cleanly() {
    let mut d = Daemon::start();
    let pid_file = d.data_dir.join("lotsed.pid");
    assert_eq!(
        std::fs::read_to_string(&pid_file).unwrap().trim(),
        d.pid().to_string()
    );

    let mut c = d.client();
    let project = c.add_project(&d.project_dir("proj"));
    let session = c.create_session(&project, Some("/bin/sh"));
    c.attach(&session.id, 80, 24);
    c.send_pty(&session.id, b"echo pid=$$\n");
    let shell = c.wait_number(&session.id, "pid=");

    c.expect_ok(Request::Shutdown);
    let status = d
        .wait_exit(Duration::from_secs(8))
        .expect("daemon did not exit");
    assert!(status.success(), "{status}");
    assert!(!d.socket().exists(), "socket file left behind");
    assert!(!pid_file.exists(), "pidfile left behind");
    assert!(wait_dead(shell), "session process outlived the daemon");
}

#[test]
fn sigterm_is_a_graceful_shutdown_too() {
    let mut d = Daemon::start();
    let mut c = d.client();
    let project = c.add_project(&d.project_dir("proj"));
    let session = c.create_session(&project, Some("/bin/sh"));
    c.attach(&session.id, 80, 24);
    c.send_pty(&session.id, b"echo pid=$$\n");
    let shell = c.wait_number(&session.id, "pid=");

    kill(Pid::from_raw(d.pid() as i32), Signal::SIGTERM).unwrap();
    let status = d
        .wait_exit(Duration::from_secs(8))
        .expect("daemon did not exit");
    assert!(status.success(), "{status}");
    assert!(!d.socket().exists());
    assert!(!d.data_dir.join("lotsed.pid").exists());
    assert!(wait_dead(shell));
}

#[test]
fn a_second_daemon_on_the_same_data_dir_refuses_to_start() {
    let d = Daemon::start();
    let out = Command::new(env!("CARGO_BIN_EXE_lotsed"))
        .arg("--data-dir")
        .arg(&d.data_dir)
        .arg("--no-login-env")
        .env_clear()
        .output()
        .unwrap();
    assert!(!out.status.success());
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(err.contains("already running"), "{err}");
    // the first one is untouched
    let mut c = d.client();
    assert!(matches!(c.request(Request::Ping), Response::Pong));
}

#[test]
fn a_stale_socket_and_pidfile_do_not_block_a_start() {
    let d = Daemon::start();
    let tmp_pid = d.pid();
    // simulate a crash: SIGKILL leaves socket and pidfile behind
    kill(Pid::from_raw(tmp_pid as i32), Signal::SIGKILL).unwrap();
    let mut d = d;
    d.wait_exit(Duration::from_secs(5)).expect("killed");
    assert!(d.socket().exists() && d.data_dir.join("lotsed.pid").exists());
    let d = d.restart();
    let mut c = d.client();
    assert!(matches!(c.request(Request::Ping), Response::Pong));
}

#[test]
fn detached_start_survives_its_launcher_and_second_start_is_a_noop() {
    struct Cleanup(Option<i32>);
    impl Drop for Cleanup {
        fn drop(&mut self) {
            if let Some(pid) = self.0 {
                let _ = kill(Pid::from_raw(pid), Signal::SIGKILL);
            }
        }
    }

    let tmp = tempfile::Builder::new().prefix("lt").tempdir().unwrap();
    let data_dir = tmp.path().join("data");
    let start = || {
        Command::new(env!("CARGO_BIN_EXE_lotsed"))
            .args(["--detach", "--no-login-env", "--data-dir"])
            .arg(&data_dir)
            .env_clear()
            .env("PATH", "/usr/bin:/bin")
            .env("HOME", tmp.path())
            .env("SHELL", "/bin/sh")
            .output()
            .unwrap()
    };
    let mut cleanup = Cleanup(None);

    let first = start();
    assert!(
        first.status.success(),
        "{}",
        String::from_utf8_lossy(&first.stderr)
    );
    let out = String::from_utf8_lossy(&first.stdout).into_owned();
    let pid: i32 = number_after(&out, "pid ").unwrap_or_else(|| panic!("no pid in {out:?}"));
    cleanup.0 = Some(pid);
    assert!(out.contains("started"), "{out}");
    assert!(pid_alive(pid), "daemon must outlive the launcher");

    // pidfile has the daemon's pid, the daemon sits in its own session
    let pid_file = std::fs::read_to_string(data_dir.join("lotsed.pid")).unwrap();
    assert_eq!(pid_file.trim(), pid.to_string());
    let pgid = Command::new("ps")
        .args(["-o", "pgid=", "-p", &pid.to_string()])
        .output()
        .unwrap();
    assert_eq!(
        String::from_utf8_lossy(&pgid.stdout).trim(),
        pid.to_string(),
        "setsid: the daemon leads its own process group"
    );

    // it answers
    let token = std::fs::read_to_string(data_dir.join("token")).unwrap();
    let mut c = Client::connect(&data_dir.join("lotsed.sock"), token.trim());
    assert!(matches!(c.request(Request::Ping), Response::Pong));

    // second detach: no new process
    let second = start();
    assert!(second.status.success());
    let out2 = String::from_utf8_lossy(&second.stdout).into_owned();
    assert!(out2.contains("already running"), "{out2}");
    assert_eq!(number_after(&out2, "pid "), Some(pid));

    // log file is private and there is no secret in it
    let log = data_dir.join("lotsed.log");
    assert_eq!(
        std::fs::metadata(&log).unwrap().permissions().mode_bits(),
        0o600
    );
    assert!(
        !std::fs::read_to_string(&log)
            .unwrap()
            .contains(token.trim())
    );

    c.expect_ok(Request::Shutdown);
    assert!(wait_dead(pid), "daemon did not stop");
    cleanup.0 = None;
}

trait ModeBits {
    fn mode_bits(&self) -> u32;
}
impl ModeBits for std::fs::Permissions {
    fn mode_bits(&self) -> u32 {
        use std::os::unix::fs::PermissionsExt;
        self.mode() & 0o777
    }
}

#[test]
fn projects_settings_and_token_survive_a_daemon_restart_sessions_do_not() {
    let d = Daemon::start();
    let token = d.token();
    let mut c = d.client();
    let project = c.add_project(&d.project_dir("keep-me"));
    let session = c.create_session(&project, Some("/bin/sh"));
    let mut settings = match c.request(Request::GetSettings) {
        Response::Settings(s) => s,
        other => panic!("{other:?}"),
    };
    settings.launch_commands.codex = "codex --fast".into();
    assert!(matches!(
        c.request(Request::UpdateSettings {
            settings: settings.clone()
        }),
        Response::Settings(_)
    ));
    c.expect_ok(Request::Shutdown);
    let mut d = d;
    d.wait_exit(Duration::from_secs(8)).expect("exit");

    let d = d.restart();
    assert_eq!(d.token(), token, "token is stable across restarts");
    let mut c = d.client();
    match c.request(Request::GetSnapshot) {
        Response::Snapshot(s) => {
            assert_eq!(s.projects, vec![project.clone()]);
            assert!(
                s.sessions.is_empty(),
                "sessions do not survive a daemon restart in Alpha: {:?}",
                s.sessions
            );
            assert_eq!(s.settings, settings);
        }
        other => panic!("{other:?}"),
    }
    // the pruned session is really gone
    assert!(matches!(
        c.request(Request::RetrySession {
            session_id: session.id
        }),
        Response::Error(_)
    ));
}
