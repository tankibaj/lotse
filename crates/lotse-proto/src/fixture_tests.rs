//! Round-trip every `fixtures/*.json` and check the fixtures can reproduce every mock state.
//! A fixture file holds one `Message` or an array of them.

use crate::*;
use serde_json::Value;
use std::collections::BTreeSet;
use std::path::PathBuf;

fn fixture_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("fixtures")
}

/// (file name, index in file, raw JSON, parsed message)
fn load_all() -> Vec<(String, usize, Value, Message)> {
    let mut files: Vec<_> = std::fs::read_dir(fixture_dir())
        .unwrap()
        .map(|e| e.unwrap().path())
        .filter(|p| p.extension().is_some_and(|e| e == "json"))
        .collect();
    files.sort();
    let mut out = Vec::new();
    for path in files {
        let name = path.file_name().unwrap().to_string_lossy().into_owned();
        let raw: Value = serde_json::from_slice(&std::fs::read(&path).unwrap())
            .unwrap_or_else(|e| panic!("{name}: invalid JSON: {e}"));
        let items = match raw {
            Value::Array(items) => items,
            single => vec![single],
        };
        for (i, v) in items.into_iter().enumerate() {
            let msg: Message = serde_json::from_value(v.clone())
                .unwrap_or_else(|e| panic!("{name}[{i}]: not a Message: {e}"));
            out.push((name.clone(), i, v, msg));
        }
    }
    out
}

#[test]
fn every_fixture_round_trips_through_json_and_frames() {
    let all = load_all();
    assert!(
        all.len() > 50,
        "expected the full fixture set, got {}",
        all.len()
    );
    for (name, i, raw, msg) in all {
        let again = serde_json::to_value(&msg).unwrap();
        assert_eq!(
            again, raw,
            "{name}[{i}]: JSON round-trip changed the message"
        );

        let frame = Frame::Control(msg);
        let bytes = encode(&frame).unwrap();
        let (back, used) = decode(&bytes).unwrap().unwrap();
        assert_eq!(used, bytes.len());
        assert_eq!(
            back, frame,
            "{name}[{i}]: frame round-trip changed the message"
        );
    }
}

#[test]
fn fixtures_cover_every_request_response_and_event() {
    let (mut methods, mut kinds, mut events) = (BTreeSet::new(), BTreeSet::new(), BTreeSet::new());
    for (_, _, _, msg) in load_all() {
        match msg {
            Message::Request(r) => {
                methods.insert(r.request.method());
            }
            Message::Response(r) => {
                kinds.insert(r.response.kind());
            }
            Message::Event(e) => {
                events.insert(e.name());
            }
            Message::Hello(_) | Message::HelloOk(_) => {}
        }
    }
    let want = |all: &[&'static str]| all.iter().copied().collect::<BTreeSet<_>>();
    assert_eq!(
        methods,
        want(Request::METHODS),
        "request methods without a fixture"
    );
    assert_eq!(
        kinds,
        want(Response::KINDS),
        "response kinds without a fixture"
    );
    assert_eq!(events, want(Event::NAMES), "events without a fixture");
}

#[test]
fn wire_tags_match_the_name_helpers() {
    for (name, i, raw, msg) in load_all() {
        let body = &raw["body"];
        match msg {
            Message::Request(r) => {
                assert_eq!(body["request"]["method"], r.request.method(), "{name}[{i}]")
            }
            Message::Response(r) => {
                assert_eq!(body["response"]["kind"], r.response.kind(), "{name}[{i}]")
            }
            Message::Event(e) => assert_eq!(body["event"], e.name(), "{name}[{i}]"),
            Message::Hello(_) | Message::HelloOk(_) => {}
        }
    }
}

#[test]
fn proto_version_in_handshake_fixtures_matches_the_constant() {
    for (_, _, _, msg) in load_all() {
        match msg {
            Message::Hello(h) => assert_eq!(h.proto_version, PROTO_VERSION),
            Message::HelloOk(h) => assert_eq!(h.proto_version, PROTO_VERSION),
            _ => {}
        }
    }
}

fn mock_snapshot() -> Snapshot {
    let raw = std::fs::read(fixture_dir().join("snapshot_mock.json")).unwrap();
    match serde_json::from_slice::<Message>(&raw).unwrap() {
        Message::Response(ResponseEnvelope {
            response: Response::Snapshot(s),
            ..
        }) => s,
        other => panic!("snapshot_mock.json is not a snapshot response: {other:?}"),
    }
}

/// `spec/mock.html` data model `P`: 5 projects, 7 sessions.
#[test]
fn snapshot_mock_reproduces_the_mock_states() {
    let s = mock_snapshot();
    assert_eq!(s.daemon.proto_version, PROTO_VERSION);
    assert_eq!(s.projects.len(), 5);
    assert_eq!(s.sessions.len(), 7);

    let rolled: Vec<_> = s
        .projects
        .iter()
        .map(|p| {
            let st = s
                .sessions
                .iter()
                .filter(|x| x.project_id == p.id)
                .flat_map(|x| {
                    std::iter::once(x.status).chain(x.sub_agents.iter().map(|a| a.status))
                });
            (p.name.as_str(), rollup(st))
        })
        .collect();
    use Status::*;
    assert_eq!(
        rolled,
        [
            ("billing-api", Run),
            ("praxis-web", Perm),
            ("infra-terraform", Run),
            ("dotfiles", Done),
            ("notes", Idle)
        ]
    );

    let session_statuses: BTreeSet<_> = s.sessions.iter().map(|x| x.status.rank()).collect();
    let sub_statuses: BTreeSet<_> = s
        .sessions
        .iter()
        .flat_map(|x| x.sub_agents.iter().map(|a| a.status.rank()))
        .collect();
    assert_eq!(
        session_statuses,
        [Idle, Done, Run, Fail, Perm].map(Status::rank).into()
    );
    assert_eq!(sub_statuses, [Done, Run, Perm].map(Status::rank).into());

    // Need-you filter: a3 (perm + sub perm) and a4 (fail).
    let need: Vec<_> = s
        .sessions
        .iter()
        .filter(|x| x.status.needs_you() || x.sub_agents.iter().any(|a| a.status.needs_you()))
        .map(|x| x.id.as_str())
        .collect();
    assert_eq!(need, ["a3", "a4"]);

    // Every agent kind and detect source the UI shows.
    let agents: BTreeSet<_> = s.sessions.iter().map(|x| x.agent.label()).collect();
    assert_eq!(
        agents,
        ["Claude Code", "Codex", "OpenCode", "Terminal"].into()
    );

    // Git line: worktree vs main checkout, ahead/dirty, and the "No git" project.
    assert!(
        s.sessions
            .iter()
            .any(|x| x.git.worktree && x.git.ahead > 0 && x.git.dirty > 0)
    );
    assert!(s.sessions.iter().any(|x| !x.git.worktree && x.git.is_git));
    let notes = s.projects.iter().find(|p| p.name == "notes").unwrap();
    assert!(!notes.is_git);
    assert!(
        s.sessions
            .iter()
            .filter(|x| x.project_id == notes.id)
            .all(|x| !x.git.is_git)
    );

    // Perm card data, running timer, unread dot.
    let a3 = s.sessions.iter().find(|x| x.id.as_str() == "a3").unwrap();
    assert_eq!(a3.perm.as_ref().unwrap().tool, "Bash");
    assert!(
        s.sessions
            .iter()
            .filter(|x| x.status == Run)
            .all(|x| x.running_since.is_some())
    );
    assert!(s.sessions.iter().any(|x| x.unread));

    // Settings defaults equal the mock's Settings screens.
    assert_eq!(s.settings, Settings::default());

    // One default workspace holds every project (the single-workspace sidebar of the mock).
    assert_eq!(s.workspaces.len(), 1);
    assert!(
        s.projects
            .iter()
            .all(|p| p.workspace_id == s.workspaces[0].id)
    );

    // Status bar: "Today 412k tokens · $4.18".
    assert_eq!(s.usage.total_tokens(), 412_000);
    assert_eq!(s.usage.cost_micros, Some(4_180_000));
}

#[test]
fn proto_version_is_5() {
    assert_eq!(PROTO_VERSION, 5);
}

/// The TypeScript mirror and the Tauri bridge rely on these exact v5 shapes.
#[test]
fn v5_messages_have_the_documented_wire_shape() {
    use serde_json::json;
    let sid = SessionId::new("a1");

    assert_eq!(
        serde_json::to_value(Request::ModAttach {
            session_id: sid.clone()
        })
        .unwrap(),
        json!({"method": "mod_attach", "params": {"session_id": "a1"}})
    );
    assert_eq!(
        serde_json::to_value(Request::ResolveSessionStart {
            id: "r-1".into(),
            approve: false
        })
        .unwrap(),
        json!({"method": "resolve_session_start", "params": {"id": "r-1", "approve": false}})
    );
    assert_eq!(
        serde_json::to_value(DetectSource::Mod).unwrap(),
        json!("mod")
    );
    assert!(
        DetectSource::Mod < DetectSource::Hook,
        "mod is the strongest source"
    );

    let ev = Event::ModCommand {
        session_id: sid.clone(),
        command: ModCommand::Answer {
            prompt_id: Some("p-1".into()),
            decision: PermDecision::Deny,
        },
    };
    assert_eq!(
        serde_json::to_value(&ev).unwrap(),
        json!({"event": "mod_command", "data": {"session_id": "a1",
            "command": {"cmd": "answer", "prompt_id": "p-1", "decision": "deny"}}})
    );
    assert_eq!(
        serde_json::to_value(ModCommand::Ping).unwrap(),
        json!({"cmd": "ping"})
    );

    let ev = Event::SessionStartRequested {
        id: "r-1".into(),
        parent_session: sid,
        project_id: ProjectId::new("p1"),
        agent: AgentKind::Codex,
        title: None,
        prompt: "go".into(),
        worktree: None,
        at: 9,
    };
    assert_eq!(
        serde_json::to_value(&ev).unwrap(),
        json!({"event": "session_start_requested", "data": {"id": "r-1", "parent_session": "a1",
            "project_id": "p1", "agent": "codex", "title": null, "prompt": "go",
            "worktree": null, "at": 9}})
    );

    let usage = UsageToday {
        rate_limits: vec![RateLimit {
            kind: "five_hour".into(),
            pct: 42,
            resets_at: 5,
        }],
        ..UsageToday::default()
    };
    assert_eq!(
        serde_json::to_value(&usage).unwrap()["rate_limits"],
        json!([{"kind": "five_hour", "pct": 42, "resets_at": 5}])
    );
}

/// M0b: the child-session setting and the withdrawn card are additive within proto 5.
#[test]
fn m0b_setting_and_withdrawn_event() {
    use serde_json::json;
    assert!(!Settings::default().agents_start_sessions_without_asking);
    assert_eq!(
        serde_json::to_value(Settings::default()).unwrap()["agents_start_sessions_without_asking"],
        json!(false)
    );

    let ev = Event::SessionStartWithdrawn { id: "r-1".into() };
    assert_eq!(ev.name(), "session_start_withdrawn");
    assert!(Event::NAMES.contains(&"session_start_withdrawn"));
    let wire = json!({"event": "session_start_withdrawn", "data": {"id": "r-1"}});
    assert_eq!(serde_json::to_value(&ev).unwrap(), wire);
    assert_eq!(serde_json::from_value::<Event>(wire).unwrap(), ev);
}

/// A v4-shaped message, without any field v5 added, still parses and takes the defaults.
#[test]
fn v4_shaped_json_without_the_new_fields_still_parses() {
    use serde_json::{from_value, json};

    let sub: SubAgent = from_value(json!({"id": "s1", "name": "x", "status": "run"})).unwrap();
    assert_eq!(
        (
            sub.parent,
            sub.activity,
            sub.tokens,
            sub.cost_usd,
            sub.model,
            sub.background
        ),
        (None, None, None, None, None, false)
    );

    let git = json!({"is_git": true, "branch": "main", "ahead": 0, "dirty": 0,
        "worktree": false, "worktree_path": null});
    let s: Session = from_value(json!({"id": "a1", "project_id": "p1", "title": "t",
        "agent": "claude_code", "command": "claude", "status": "run", "sub_agents": [],
        "git": git, "created_at": 1, "last_activity": 2, "running_since": null,
        "unread": false, "perm": null, "source": "hook"}))
    .unwrap();
    assert_eq!(
        (s.context_pct, s.source_layer, s.parent_session, s.todo),
        (None, None, None, None)
    );

    let u: UsageToday = from_value(json!({"input_tokens": 1, "output_tokens": 2,
        "cache_read_tokens": 3, "cache_write_tokens": 4, "cost_micros": null}))
    .unwrap();
    assert!(u.rate_limits.is_empty());

    let n: NewSession = from_value(
        json!({"project_id": "p1", "agent": "codex", "command": null,
        "title": null, "worktree": null, "first_prompt": null}),
    )
    .unwrap();
    assert_eq!((n.parent_session, n.requested_by_agent), (None, false));
}

/// The TypeScript mirror and the Tauri bridge rely on these exact v4 shapes.
#[test]
fn v4_messages_have_the_documented_wire_shape() {
    use serde_json::json;
    let sid = SessionId::new("a1");

    let req = Request::GetDiff {
        session_id: sid.clone(),
        base: DiffBase::MergeBase,
    };
    assert_eq!(
        serde_json::to_value(&req).unwrap(),
        json!({"method": "get_diff", "params": {"session_id": "a1", "base": "merge_base"}})
    );
    let req = Request::ListFiles {
        session_id: sid.clone(),
        changed_only: true,
    };
    assert_eq!(
        serde_json::to_value(&req).unwrap(),
        json!({"method": "list_files", "params": {"session_id": "a1", "changed_only": true}})
    );
    let req = Request::ResolvePermission {
        session_id: sid.clone(),
        decision: PermDecision::Allow,
        via: Some(PermVia::Notification),
        prompt_id: Some("p-1".into()),
    };
    assert_eq!(
        serde_json::to_value(&req).unwrap(),
        json!({"method": "resolve_permission", "params": {"session_id": "a1", "decision": "allow",
            "via": "notification", "prompt_id": "p-1"}})
    );
    let req = Request::ExportTranscript {
        session_id: sid.clone(),
        format: ExportFormat::JsonLines,
    };
    assert_eq!(
        serde_json::to_value(&req).unwrap(),
        json!({"method": "export_transcript", "params": {"session_id": "a1", "format": "json_lines"}})
    );

    let resp = Response::Connectors(vec![Connector {
        agent: AgentKind::ClaudeCode,
        name: "github".into(),
        transport: "stdio".into(),
        config_path: "/p/.mcp.json".into(),
        status: ConnectorStatus::Connected { tools: 12 },
    }]);
    assert_eq!(
        serde_json::to_value(&resp).unwrap(),
        json!({"kind": "connectors", "data": [{"agent": "claude_code", "name": "github",
            "transport": "stdio", "config_path": "/p/.mcp.json",
            "status": {"state": "connected", "tools": 12}}]})
    );
    let failed = serde_json::to_value(ConnectorStatus::Failed {
        reason: "timeout".into(),
    })
    .unwrap();
    assert_eq!(failed, json!({"state": "failed", "reason": "timeout"}));
    assert_eq!(
        serde_json::to_value(ConnectorStatus::NeedsAuth).unwrap(),
        json!({"state": "needs_auth"})
    );

    let ev = Event::Notification {
        session_id: sid,
        kind: NotificationKind::Perm,
        title: "t".into(),
        subtitle: "s".into(),
        body: None,
        banner: false,
        sound: true,
        sound_name: Some("Glass".into()),
        prompt_id: Some("p-1".into()),
        at: 7,
    };
    assert_eq!(
        serde_json::to_value(&ev).unwrap(),
        json!({"event": "notification", "data": {"session_id": "a1", "kind": "perm", "title": "t",
            "subtitle": "s", "body": null, "banner": false, "sound": true, "sound_name": "Glass",
            "prompt_id": "p-1", "at": 7}})
    );
}

/// QA: a v3-shaped message, without any field v4 added, still parses and takes the defaults.
#[test]
fn v3_shaped_json_without_the_new_fields_still_parses() {
    use serde_json::{from_value, json};

    let git: GitInfo = from_value(json!({"is_git": true, "branch": "main", "ahead": 1,
        "dirty": 2, "worktree": false, "worktree_path": null}))
    .unwrap();
    assert_eq!(
        (git.behind, git.added, git.removed, git.pr),
        (0, 0, 0, None)
    );

    let perm: PermRequest = from_value(json!({"tool": "Bash", "summary": "ls"})).unwrap();
    assert_eq!(perm.id, None);

    let req: Request = from_value(
        json!({"method": "resolve_permission", "params": {"session_id": "a1", "decision": "deny"}}),
    )
    .unwrap();
    assert_eq!(
        req,
        Request::ResolvePermission {
            session_id: SessionId::new("a1"),
            decision: PermDecision::Deny,
            via: None,
            prompt_id: None,
        }
    );
    let req: Request =
        from_value(json!({"method": "get_diff", "params": {"session_id": "a1"}})).unwrap();
    assert_eq!(
        req,
        Request::GetDiff {
            session_id: SessionId::new("a1"),
            base: DiffBase::Head,
        }
    );

    let file: DiffFile = from_value(
        json!({"path": "a.rs", "old_path": null, "status": "modified",
        "binary": false, "truncated": false, "diff": ""}),
    )
    .unwrap();
    assert_eq!((file.added, file.removed), (0, 0));

    let turn: TranscriptTurn = from_value(json!({"role": "tool", "text": "", "at": null,
            "tool": {"name": "Bash", "summary": "ls"}}))
    .unwrap();
    assert_eq!((turn.approval, turn.sub_agent), (None, None));
    let tool = turn.tool.unwrap();
    assert_eq!((tool.status, tool.result), (None, None));
    let t: Transcript =
        from_value(json!({"available": true, "turns": [], "truncated": false})).unwrap();
    assert_eq!(t.source, TranscriptSource::Claude);

    let ev: Event = from_value(json!({"event": "notification", "data": {"session_id": "a1",
        "kind": "done", "title": "t", "subtitle": "s", "body": null, "sound": false, "at": 1}}))
    .unwrap();
    let Event::Notification {
        banner,
        sound_name,
        prompt_id,
        ..
    } = ev
    else {
        unreachable!()
    };
    assert!(banner, "a v3 notification is always a banner");
    assert_eq!((sound_name, prompt_id), (None, None));

    let prefs: EventPrefs =
        from_value(json!({"banner": true, "sound": true, "allow_deny": false})).unwrap();
    assert_eq!(prefs.sound_name, None);
    let v3_settings = json!({
        "launch_commands": {"terminal": "$SHELL -l", "claude_code": "claude", "codex": "codex",
            "opencode": "opencode"},
        "notifications": {
            "perm": {"banner": true, "sound": true, "allow_deny": true},
            "fail": {"banner": true, "sound": true, "allow_deny": false},
            "done": {"banner": true, "sound": false, "allow_deny": false},
            "sub_agent_done": {"banner": false, "sound": false, "allow_deny": false},
            "only_when_unfocused": true, "paused": false},
        "appearance": {"theme": "auto", "interface_font": "System (SF Pro)",
            "interface_font_size": 13, "terminal_font": "SF Mono", "terminal_font_size": 13},
        "scrollback_bytes": 4194304
    });
    let s: Settings = from_value(v3_settings).unwrap();
    assert_eq!(s, Settings::default());
    assert!(
        !s.external.pr_status && !s.external.mcp_live,
        "opt-in: off by default"
    );
}

/// The TypeScript mirror and the Tauri bridge rely on these exact shapes.
#[test]
fn v3_messages_have_the_documented_wire_shape() {
    use serde_json::json;
    let req = Request::DeleteWorkspace {
        workspace_id: WorkspaceId::new("w2"),
        move_projects: true,
    };
    assert_eq!(
        serde_json::to_value(&req).unwrap(),
        json!({"method": "delete_workspace", "params": {"workspace_id": "w2", "move_projects": true}})
    );
    let resp = Response::Workspace(Workspace {
        id: WorkspaceId::new("w2"),
        name: "Client work".into(),
    });
    assert_eq!(
        serde_json::to_value(&resp).unwrap(),
        json!({"kind": "workspace", "data": {"id": "w2", "name": "Client work"}})
    );
    let ev = Event::UsageChanged(UsageToday {
        input_tokens: 1,
        output_tokens: 2,
        cache_read_tokens: 3,
        cache_write_tokens: 4,
        cost_micros: None,
        rate_limits: Vec::new(),
    });
    assert_eq!(
        serde_json::to_value(&ev).unwrap(),
        json!({"event": "usage_changed", "data": {"input_tokens": 1, "output_tokens": 2,
            "cache_read_tokens": 3, "cache_write_tokens": 4, "cost_micros": null}})
    );
    assert_eq!(UsageToday::default().total_tokens(), 0);
}

#[test]
fn launch_command_defaults_match_the_mock() {
    let d: Vec<_> = AgentKind::ALL
        .iter()
        .map(|a| (a.label(), a.default_command()))
        .collect();
    assert_eq!(
        d,
        [
            ("Terminal", "$SHELL -l"),
            ("Claude Code", "claude"),
            ("Codex", "codex"),
            ("OpenCode", "opencode")
        ]
    );
}
