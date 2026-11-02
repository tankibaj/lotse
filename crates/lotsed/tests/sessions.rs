//! Done-when flow of WP1 over the real socket: project → spawn → input → output → detach →
//! reattach replays → kill; clients come and go, sessions stay.

mod common;

use common::*;
use lotse_proto::{ErrorCode, Event, Request, Response, SessionId, Status};
use std::time::Duration;

const ECHO: &[u8] = b"echo lotse-$((20+22))\n";

fn error_code(r: Response) -> ErrorCode {
    match r {
        Response::Error(e) => e.code,
        other => panic!("expected error, got {other:?}"),
    }
}

#[test]
fn full_flow_create_spawn_echo_detach_reattach_replays_kill() {
    let d = Daemon::start();
    let mut c = d.client();
    let project = c.add_project(&d.project_dir("proj"));
    let session = c.create_session(&project, Some("/bin/sh"));
    assert_eq!(session.status, Status::Idle);
    assert_eq!(session.project_id, project.id);

    // spawn → write `echo hi` → read `hi`
    c.attach(&session.id, 80, 24);
    c.send_pty(&session.id, ECHO);
    assert!(
        c.wait_output(&session.id, "lotse-42"),
        "output: {:?}",
        c.output_text(&session.id)
    );

    // detach: no more frames for this client
    c.expect_ok(Request::Detach {
        session_id: session.id.clone(),
    });
    c.clear_output();
    let other = &mut d.client();
    other.send_pty(&session.id, b"echo while-away-$((1+1))\n");
    c.drain(Duration::from_millis(500));
    assert!(
        c.output.get(&session.id).is_none_or(Vec::is_empty),
        "frames after detach"
    );

    // reattach: replay holds both the old and the new output, nothing else arrives out of order
    let replay_len = c.attach(&session.id, 80, 24);
    let text = c.output_text(&session.id);
    assert_eq!(text.len() as u64, replay_len);
    assert!(text.contains("lotse-42"), "replay: {text:?}");
    assert!(text.contains("while-away-2"), "replay: {text:?}");

    // kill
    c.expect_ok(Request::CloseSession {
        session_id: session.id.clone(),
    });
    c.wait_event(|e| matches!(e, Event::SessionClosed { session_id } if *session_id == session.id));
    match c.request(Request::ListSessions { project_id: None }) {
        Response::Sessions(s) => assert!(s.is_empty()),
        other => panic!("{other:?}"),
    }
    assert_eq!(
        error_code(c.request(Request::CloseSession {
            session_id: session.id.clone()
        })),
        ErrorCode::NotFound
    );
}

#[test]
fn closing_a_session_kills_its_whole_process_group() {
    let d = Daemon::start();
    let mut c = d.client();
    let project = c.add_project(&d.project_dir("proj"));
    let session = c.create_session(&project, Some("/bin/sh"));
    c.attach(&session.id, 80, 24);
    c.send_pty(&session.id, b"sleep 300 & echo shellpid=$$ bgpid=$!\n");
    let shell = c.wait_number(&session.id, "shellpid=");
    let bg = c.wait_number(&session.id, "bgpid=");
    assert!(pid_alive(shell) && pid_alive(bg));

    c.expect_ok(Request::CloseSession {
        session_id: session.id.clone(),
    });
    assert!(wait_dead(shell), "shell still alive");
    assert!(wait_dead(bg), "background job still alive");
}

#[test]
fn a_foreground_job_that_ignores_sighup_is_killed_after_the_grace_period() {
    let d = Daemon::start();
    let mut c = d.client();
    let project = c.add_project(&d.project_dir("proj"));
    let session = c.create_session(&project, Some("/bin/sh"));
    c.attach(&session.id, 80, 24);
    // the job runs in its own foreground group (job control) and ignores SIGHUP, like `nohup`
    c.send_pty(
        &session.id,
        b"sh -c 'trap \"\" HUP; echo fgpid=$$; exec sleep 300'\n",
    );
    let job = c.wait_number(&session.id, "fgpid=");
    assert!(pid_alive(job));

    let closed = std::time::Instant::now();
    c.expect_ok(Request::CloseSession {
        session_id: session.id.clone(),
    });
    assert!(wait_dead(job), "SIGHUP-immune job survived the session");
    assert!(
        closed.elapsed() >= Duration::from_millis(1500),
        "it died to SIGKILL after the grace period, not to SIGHUP ({:?})",
        closed.elapsed()
    );
}

#[test]
fn closing_a_session_whose_shell_already_exited_still_cleans_up_its_group() {
    let d = Daemon::start();
    let mut c = d.client();
    let project = c.add_project(&d.project_dir("proj"));
    let session = c.create_session(&project, Some("/bin/sh"));
    c.attach(&session.id, 80, 24);
    // no job control: the background job stays in the shell's own process group; it ignores SIGHUP,
    // so neither the kernel's hangup at shell exit nor ours can remove it, only the SIGKILL can
    c.send_pty(&session.id, b"set +m; trap '' HUP\n");
    c.send_pty(&session.id, b"sleep 300 & echo leftover=$!\n");
    let bg = c.wait_number(&session.id, "leftover=");
    c.send_pty(&session.id, b"exit\n");
    c.wait_event(|e| matches!(e, Event::SessionExited { .. }));
    assert!(pid_alive(bg), "the leftover outlives the shell");

    c.expect_ok(Request::CloseSession {
        session_id: session.id.clone(),
    });
    assert!(
        wait_dead(bg),
        "leftover of an exited session survived close"
    );
}

#[test]
fn a_command_that_exits_at_once_is_announced_before_its_exit() {
    let d = Daemon::start();
    let mut watcher = d.client();
    let mut c = d.client();
    let project = c.add_project(&d.project_dir("proj"));
    let session = c.create_session(&project, Some("/usr/bin/false"));
    watcher.drain(Duration::from_millis(1200));
    let pos = |pred: &dyn Fn(&Event) -> bool| watcher.events.iter().position(pred);
    let created = pos(&|e| matches!(e, Event::SessionCreated(s) if s.id == session.id));
    let exited = pos(&|e| {
        matches!(
            e,
            Event::SessionExited {
                exit_code: Some(1),
                ..
            }
        )
    });
    let status = pos(&|e| {
        matches!(
            e,
            Event::StatusChanged {
                status: Status::Fail,
                ..
            }
        )
    });
    let (created, exited, status) = (
        created.expect("session_created"),
        exited.expect("session_exited"),
        status.expect("status_changed"),
    );
    assert!(created < exited && exited < status, "{:?}", watcher.events);
    match c.request(Request::ListSessions { project_id: None }) {
        Response::Sessions(s) => assert_eq!(s[0].status, Status::Fail),
        other => panic!("{other:?}"),
    }
}

#[test]
fn attaching_while_output_streams_loses_and_duplicates_nothing() {
    let d = Daemon::start();
    let mut a = d.client();
    let project = a.add_project(&d.project_dir("proj"));
    let session = a.create_session(&project, Some("/bin/sh"));
    // ~6 MB of numbered lines (more than the 4 MiB ring), typed by a client that is not attached.
    // (BSD `seq` prints 1e+06 from a million on, so stay below.)
    a.send_pty(&session.id, b"seq 1 900000; echo end-$((1+1))\n");
    std::thread::sleep(Duration::from_millis(50));

    let mut b = d.client();
    b.attach(&session.id, 120, 30); // also asserts: `attached` comes before any byte
    assert!(
        b.wait_output(&session.id, "end-2\r"),
        "stream did not finish"
    );
    let text = b.output_text(&session.id);
    let nums: Vec<u64> = text
        .lines()
        .filter_map(|l| l.trim_end_matches('\r').parse().ok())
        .collect();
    assert!(nums.len() > 10_000, "only {} numbered lines", nums.len());
    // the first line may be cut by the ring; after it every number follows its predecessor
    for w in nums[1..].windows(2) {
        assert_eq!(w[1], w[0] + 1, "gap or duplicate around {}", w[0]);
    }
    assert_eq!(*nums.last().unwrap(), 900_000);
}

#[test]
fn a_client_that_stops_reading_is_disconnected_instead_of_buffered_forever() {
    use lotse_proto::{Frame, Message, PROTO_VERSION, RequestEnvelope, read_frame, write_frame};
    use std::os::unix::net::UnixStream;

    let d = Daemon::start();
    let mut a = d.client();
    let project = a.add_project(&d.project_dir("proj"));
    let session = a.create_session(&project, Some("/bin/sh"));

    // a raw client that attaches and then never reads
    let mut slow = UnixStream::connect(d.socket()).unwrap();
    hello(&mut slow, &d.token(), PROTO_VERSION);
    assert!(matches!(
        read_frame(&mut slow).unwrap(),
        Some(Frame::Control(Message::HelloOk(_)))
    ));
    write_frame(
        &mut slow,
        &Frame::Control(Message::Request(RequestEnvelope {
            req_id: 1,
            request: Request::Attach {
                session_id: session.id.clone(),
                cols: 80,
                rows: 24,
            },
        })),
    )
    .unwrap();
    assert!(matches!(
        read_frame(&mut slow).unwrap(),
        Some(Frame::Control(Message::Response(_)))
    ));

    // 200 MB of output: far more than the daemon is willing to queue (64 MiB) for one client
    a.send_pty(
        &session.id,
        b"head -c 200000000 /dev/zero | tr '\\0' x; echo\n",
    );
    std::thread::sleep(Duration::from_secs(5));

    // (macOS refuses the option once the peer closed the socket, which is what we expect)
    let _ = slow.set_read_timeout(Some(Duration::from_secs(15)));
    let mut disconnected = false;
    loop {
        match read_frame(&mut slow) {
            Ok(Some(_)) => {}
            Ok(None) => {
                disconnected = true;
                break;
            }
            Err(lotse_proto::FrameError::Io(e))
                if matches!(
                    e.kind(),
                    std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                ) =>
            {
                break;
            }
            Err(_) => {
                // cut mid-frame or reset: the daemon closed the connection
                disconnected = true;
                break;
            }
        }
    }
    assert!(disconnected, "the slow client was never disconnected");

    // everyone else is unaffected and the session still works
    assert!(matches!(a.request(Request::Ping), Response::Pong));
    a.send_pty(&session.id, b"\x03"); // Ctrl-C ends the flood
    a.send_pty(&session.id, b"echo alive-$((2+3))\n");
    a.attach(&session.id, 80, 24);
    assert!(a.wait_output(&session.id, "alive-5\r"));
}

#[test]
fn client_restart_keeps_the_session_and_its_output() {
    let d = Daemon::start();
    let (session_id, project_id) = {
        let mut a = d.client();
        let project = a.add_project(&d.project_dir("proj"));
        let session = a.create_session(&project, Some("/bin/sh"));
        a.attach(&session.id, 80, 24);
        a.send_pty(&session.id, ECHO);
        assert!(a.wait_output(&session.id, "lotse-42"));
        (session.id, project.id)
        // client A goes away here (socket closed)
    };

    let mut b = d.client();
    match b.request(Request::GetSnapshot) {
        Response::Snapshot(s) => {
            assert_eq!(s.projects.len(), 1);
            assert_eq!(s.sessions.len(), 1);
            assert_eq!(s.sessions[0].id, session_id);
            assert_eq!(s.sessions[0].project_id, project_id);
            assert_eq!(s.daemon.pid, d.pid());
        }
        other => panic!("{other:?}"),
    }
    b.attach(&session_id, 100, 30);
    assert!(
        b.output_text(&session_id).contains("lotse-42"),
        "replay after reconnect"
    );
    // and the session still runs
    b.send_pty(&session_id, b"echo still-$((3+4))\n");
    assert!(b.wait_output(&session_id, "still-7"));
}

#[test]
fn two_clients_see_the_same_output_and_the_same_events() {
    let d = Daemon::start();
    let mut a = d.client();
    let mut b = d.client();
    let project = a.add_project(&d.project_dir("proj"));
    b.wait_event(|e| matches!(e, Event::ProjectAdded(p) if p.id == project.id));
    let session = a.create_session(&project, Some("/bin/sh"));
    b.wait_event(|e| matches!(e, Event::SessionCreated(s) if s.id == session.id));

    a.attach(&session.id, 80, 24);
    b.attach(&session.id, 80, 24);
    a.send_pty(&session.id, ECHO);
    assert!(a.wait_output(&session.id, "lotse-42"));
    assert!(b.wait_output(&session.id, "lotse-42"));
    // either client may type
    b.send_pty(&session.id, b"echo from-b-$((5+5))\n");
    assert!(a.wait_output(&session.id, "from-b-10"));
}

#[test]
fn resize_reaches_the_pty() {
    let d = Daemon::start();
    let mut c = d.client();
    let project = c.add_project(&d.project_dir("proj"));
    let session = c.create_session(&project, Some("/bin/sh"));
    c.attach(&session.id, 100, 30);
    c.send_pty(&session.id, b"stty size\n");
    assert!(
        c.wait_output(&session.id, "30 100"),
        "{:?}",
        c.output_text(&session.id)
    );
    c.expect_ok(Request::Resize {
        session_id: session.id.clone(),
        cols: 120,
        rows: 40,
    });
    // ask again until the shell answers: under heavy parallel load readline may drop a line typed
    // right after SIGWINCH, but a resize that never applied would keep answering "30 100"
    let mut seen = false;
    for _ in 0..5 {
        c.send_pty(&session.id, b"stty size\n");
        let s = session.id.clone();
        if c.pump_until(Duration::from_secs(2), |c| {
            contains(c.output.get(&s).map_or(&[][..], |v| v), b"40 120")
        }) {
            seen = true;
            break;
        }
    }
    assert!(seen, "{:?}", c.output_text(&session.id));
    assert_eq!(
        error_code(c.request(Request::Resize {
            session_id: session.id.clone(),
            cols: 0,
            rows: 10
        })),
        ErrorCode::InvalidRequest
    );
}

#[test]
fn session_starts_in_the_project_folder_with_lotse_env() {
    let d = Daemon::start();
    let mut c = d.client();
    let dir = d.project_dir("proj-env");
    let project = c.add_project(&dir);
    let session = c.create_session(&project, Some("/bin/sh"));
    c.attach(&session.id, 120, 30);
    c.send_pty(
        &session.id,
        b"echo cwd=$(pwd -P) sid=$LOTSE_SESSION_ID term=$TERM tok=${LOTSE_TOKEN:-none}\n",
    );
    assert!(
        c.wait_output(&session.id, "tok=none"),
        "{:?}",
        c.output_text(&session.id)
    );
    let text = c.output_text(&session.id);
    let canonical = std::fs::canonicalize(&dir).unwrap();
    assert!(
        text.contains(&format!("cwd={}", canonical.display())),
        "{text:?}"
    );
    assert!(text.contains(&format!("sid={}", session.id)), "{text:?}");
    assert!(text.contains("term=xterm-256color"), "{text:?}");
    assert!(
        !text.contains(&d.token()),
        "the token must never reach a session"
    );
}

#[test]
fn exit_reports_status_and_retry_restarts_in_place() {
    let d = Daemon::start();
    let mut c = d.client();
    let project = c.add_project(&d.project_dir("proj"));
    let session = c.create_session(&project, Some("/bin/sh"));
    c.attach(&session.id, 80, 24);
    assert_eq!(
        error_code(c.request(Request::RetrySession {
            session_id: session.id.clone()
        })),
        ErrorCode::Conflict,
        "retry while running"
    );

    c.send_pty(&session.id, b"exit 3\n");
    c.wait_event(|e| matches!(e, Event::SessionExited { session_id, exit_code: Some(3) } if *session_id == session.id));
    c.wait_event(|e| {
        matches!(
            e,
            Event::StatusChanged {
                status: Status::Fail,
                ..
            }
        )
    });
    match c.request(Request::ListSessions { project_id: None }) {
        Response::Sessions(s) => assert_eq!(s[0].status, Status::Fail),
        other => panic!("{other:?}"),
    }

    match c.request(Request::RetrySession {
        session_id: session.id.clone(),
    }) {
        Response::Session(s) => assert_eq!(s.status, Status::Idle),
        other => panic!("{other:?}"),
    }
    c.send_pty(&session.id, b"echo again-$((6+6))\n");
    assert!(
        c.wait_output(&session.id, "again-12"),
        "{:?}",
        c.output_text(&session.id)
    );

    // a clean exit is `done`
    c.send_pty(&session.id, b"exit 0\n");
    c.wait_event(|e| {
        matches!(
            e,
            Event::StatusChanged {
                status: Status::Done,
                ..
            }
        )
    });
}

#[test]
fn ring_buffer_is_bounded_by_the_scrollback_setting() {
    let d = Daemon::start();
    let mut c = d.client();
    let mut settings = match c.request(Request::GetSettings) {
        Response::Settings(s) => s,
        other => panic!("{other:?}"),
    };
    settings.scrollback_bytes = 64 * 1024;
    assert!(matches!(
        c.request(Request::UpdateSettings { settings }),
        Response::Settings(_)
    ));

    let project = c.add_project(&d.project_dir("proj"));
    let session = c.create_session(&project, Some("/bin/sh"));
    c.attach(&session.id, 80, 24);
    c.send_pty(
        &session.id,
        b"head -c 300000 /dev/zero | tr '\\0' 'x'; echo; echo done-$((1+1))\n",
    );
    assert!(c.wait_output(&session.id, "done-2"));
    c.expect_ok(Request::Detach {
        session_id: session.id.clone(),
    });
    assert_eq!(c.attach(&session.id, 80, 24), 64 * 1024);
}

#[test]
fn title_comes_from_the_first_prompt_and_can_be_edited() {
    let d = Daemon::start();
    let mut c = d.client();
    let project = c.add_project(&d.project_dir("proj"));
    let session = c.create_session(&project, Some("/bin/sh"));
    assert_eq!(session.title, "New session");
    c.attach(&session.id, 80, 24);

    c.send_pty(&session.id, b"echo fix the login bug\r");
    let ev = c.wait_event(|e| matches!(e, Event::SessionUpdated(s) if s.id == session.id));
    let Event::SessionUpdated(s) = ev else {
        unreachable!()
    };
    assert_eq!(s.title, "echo fix the login bug");

    // a second prompt does not replace it
    c.send_pty(&session.id, b"echo second\r");
    assert!(c.wait_output(&session.id, "second"));
    c.expect_ok(Request::RenameSession {
        session_id: session.id.clone(),
        title: "  Login bug  ".into(),
    });
    c.wait_event(|e| matches!(e, Event::SessionUpdated(s) if s.title == "Login bug"));
    c.send_pty(&session.id, b"echo third\r");
    assert!(c.wait_output(&session.id, "third"));
    match c.request(Request::ListSessions { project_id: None }) {
        Response::Sessions(s) => assert_eq!(s[0].title, "Login bug"),
        other => panic!("{other:?}"),
    }
    assert_eq!(
        error_code(c.request(Request::RenameSession {
            session_id: session.id.clone(),
            title: "  \n ".into()
        })),
        ErrorCode::InvalidRequest
    );
}

#[test]
fn first_prompt_of_a_new_session_becomes_title_and_is_typed() {
    let d = Daemon::start();
    let mut c = d.client();
    let project = c.add_project(&d.project_dir("proj"));
    let resp = c.request(Request::CreateSession(lotse_proto::NewSession {
        project_id: project.id.clone(),
        agent: lotse_proto::AgentKind::Terminal,
        command: Some("/bin/sh".into()),
        title: None,
        worktree: None,
        first_prompt: Some("echo refactor the parser".into()),
    }));
    let Response::Session(s) = resp else {
        panic!("{resp:?}")
    };
    assert_eq!(s.title, "echo refactor the parser");
    c.attach(&s.id, 80, 24);
    assert!(
        c.wait_output(&s.id, "refactor the parser"),
        "{:?}",
        c.output_text(&s.id)
    );
}

#[test]
fn create_session_errors() {
    let d = Daemon::start();
    let mut c = d.client();
    let project = c.add_project(&d.project_dir("proj"));
    assert_eq!(
        error_code(c.request(Request::CreateSession(lotse_proto::NewSession {
            project_id: "p-nope".into(),
            agent: lotse_proto::AgentKind::Terminal,
            command: None,
            title: None,
            worktree: None,
            first_prompt: None,
        }))),
        ErrorCode::NotFound
    );
    let mut new = |command: &str, worktree: bool| {
        c.request(Request::CreateSession(lotse_proto::NewSession {
            project_id: project.id.clone(),
            agent: lotse_proto::AgentKind::ClaudeCode,
            command: Some(command.into()),
            title: None,
            worktree: worktree.then(|| lotse_proto::WorktreeSpec {
                branch: "lotse/x".into(),
            }),
            first_prompt: None,
        }))
    };
    assert_eq!(
        error_code(new("lotse-no-such-agent --x", false)),
        ErrorCode::Unavailable
    );
    assert_eq!(
        error_code(new("claude 'unterminated", false)),
        ErrorCode::InvalidRequest
    );
    assert_eq!(error_code(new("/bin/sh", true)), ErrorCode::Unavailable);
    // none of the failures left a session behind
    match c.request(Request::ListSessions { project_id: None }) {
        Response::Sessions(s) => assert!(s.is_empty()),
        other => panic!("{other:?}"),
    }
}

#[test]
fn unknown_session_ids_are_not_found() {
    let d = Daemon::start();
    let mut c = d.client();
    let ghost = SessionId::new("s-ghost");
    for req in [
        Request::Attach {
            session_id: ghost.clone(),
            cols: 80,
            rows: 24,
        },
        Request::Detach {
            session_id: ghost.clone(),
        },
        Request::SendInput {
            session_id: ghost.clone(),
            text: "x".into(),
        },
        Request::MarkRead {
            session_id: ghost.clone(),
        },
        Request::RetrySession {
            session_id: ghost.clone(),
        },
    ] {
        assert_eq!(error_code(c.request(req)), ErrorCode::NotFound);
    }
    // PTY input for a ghost is ignored, the connection stays healthy
    c.send_pty(&ghost, b"x");
    assert!(matches!(c.request(Request::Ping), Response::Pong));
}

#[test]
fn projects_crud_and_session_cleanup() {
    let d = Daemon::start();
    let mut c = d.client();
    let dir = d.project_dir("billing");
    let p = c.add_project(&dir);
    assert_eq!(p.name, "billing");
    assert!(!p.is_git);
    assert_eq!(p.workspace_id.as_str(), "w1");

    // duplicates, relative and bad paths
    for (path, code) in [
        (dir.to_string_lossy().into_owned(), ErrorCode::Conflict),
        ("relative/dir".into(), ErrorCode::InvalidRequest),
        ("/definitely/not/here".into(), ErrorCode::NotFound),
        ("/etc/hosts".into(), ErrorCode::InvalidRequest),
    ] {
        let r = c.request(Request::AddProject(lotse_proto::NewProject {
            path,
            name: None,
            color: None,
            icon: None,
        }));
        assert_eq!(error_code(r), code);
    }

    // a repo is detected
    let repo = d.project_dir("repo");
    std::fs::create_dir_all(repo.join(".git")).unwrap();
    assert!(c.add_project(&repo).is_git);

    let patch = lotse_proto::ProjectPatch {
        name: Some("Billing API".into()),
        color: Some("purple".into()),
        notifications: Some(lotse_proto::NotificationMode::Mute),
        ..Default::default()
    };
    match c.request(Request::UpdateProject {
        project_id: p.id.clone(),
        patch,
    }) {
        Response::Project(u) => {
            assert_eq!(u.name, "Billing API");
            assert_eq!(u.color, "purple");
            assert_eq!(u.notifications, lotse_proto::NotificationMode::Mute);
        }
        other => panic!("{other:?}"),
    }
    let bad = lotse_proto::ProjectPatch {
        color: Some("mauve".into()),
        ..Default::default()
    };
    assert_eq!(
        error_code(c.request(Request::UpdateProject {
            project_id: p.id.clone(),
            patch: bad
        })),
        ErrorCode::InvalidRequest
    );

    // removing a project closes its sessions
    let session = c.create_session(&p, Some("/bin/sh"));
    c.attach(&session.id, 80, 24);
    c.send_pty(&session.id, b"echo pid=$$\n");
    let pid = c.wait_number(&session.id, "pid=");
    c.expect_ok(Request::RemoveProject {
        project_id: p.id.clone(),
    });
    c.wait_event(|e| matches!(e, Event::SessionClosed { session_id } if *session_id == session.id));
    c.wait_event(|e| matches!(e, Event::ProjectRemoved { project_id } if *project_id == p.id));
    assert!(wait_dead(pid));
    match c.request(Request::ListProjects) {
        Response::Projects(v) => assert_eq!(v.len(), 1),
        other => panic!("{other:?}"),
    }
}
