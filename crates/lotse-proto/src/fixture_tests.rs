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
fn proto_version_is_3() {
    assert_eq!(PROTO_VERSION, 3);
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
