//! Beta wp06: the history database (v3) and the recorder on the hub. In-process `State`, temp
//! dirs, temp HOME: nothing of the real `~/.claude` or Lotse data is touched.

mod common;

use lotse_proto::{
    AgentKind, DetectSource, Event, NewProject, NewSession, PermDecision, PermRequest, PermVia,
    Request, Response, Session, SessionId, Status, SubAgent, SubAgentId,
};
use lotsed::config::Config;
use lotsed::db::{Db, PermEventRow, SpanRow};
use lotsed::history;
use lotsed::state::State;
use rusqlite::Connection;
use serde_json::json;
use std::path::Path;
use std::sync::Arc;
use std::time::{Duration, Instant};

const DAY_MS: u64 = 86_400_000;

fn state_on(db: Db, root: &Path) -> Arc<State> {
    let home = root.join("home");
    std::fs::create_dir_all(&home).unwrap();
    let mut cfg = Config::new(root.join("data"), false);
    cfg.home = Some(home);
    State::new(cfg, db).unwrap()
}

/// A state with the recorder installed on its hub.
fn recording_state(db: Db, root: &Path) -> Arc<State> {
    let state = state_on(db, root);
    history::install(&state.hub, state.db().clone());
    state
}

fn status_event(session: &str, status: Status, at: u64) -> Event {
    Event::StatusChanged {
        session_id: SessionId::new(session),
        status,
        source: DetectSource::Hook,
        perm: None,
        running_since: None,
        at,
    }
}

fn perm_event(session: &str, prompt_id: &str, at: u64) -> Event {
    Event::StatusChanged {
        session_id: SessionId::new(session),
        status: Status::Perm,
        source: DetectSource::Hook,
        perm: Some(PermRequest {
            tool: "Bash".into(),
            summary: "ls".into(),
            always: None,
            id: Some(prompt_id.into()),
        }),
        running_since: None,
        at,
    }
}

fn lead(spans: &[SpanRow]) -> Vec<(Status, u64, Option<u64>)> {
    spans
        .iter()
        .filter(|s| s.sub_agent_id.is_none())
        .map(|s| (s.status, s.from_ms, s.to_ms))
        .collect()
}

// the v2 schema as the gap branch left it (what a user's database looks like before this change)
const V2_SCHEMA: &str = "
CREATE TABLE workspaces (id TEXT PRIMARY KEY, name TEXT NOT NULL, created_at INTEGER NOT NULL);
CREATE TABLE projects (
    id TEXT PRIMARY KEY,
    workspace_id TEXT NOT NULL REFERENCES workspaces(id) ON DELETE CASCADE,
    name TEXT NOT NULL, path TEXT NOT NULL UNIQUE, color TEXT NOT NULL, icon TEXT NOT NULL,
    is_git INTEGER NOT NULL, notifications TEXT NOT NULL, created_at INTEGER NOT NULL);
CREATE TABLE sessions (
    id TEXT PRIMARY KEY,
    project_id TEXT NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    title TEXT NOT NULL, title_auto INTEGER NOT NULL, agent TEXT NOT NULL, command TEXT NOT NULL,
    created_at INTEGER NOT NULL, last_activity INTEGER NOT NULL);
CREATE TABLE settings (key TEXT PRIMARY KEY, value TEXT NOT NULL);
CREATE UNIQUE INDEX workspaces_name ON workspaces(name COLLATE NOCASE);
CREATE TABLE transcripts (path TEXT PRIMARY KEY, seen_at INTEGER NOT NULL);";

fn user_version(c: &Connection) -> u32 {
    c.pragma_query_value(None, "user_version", |r| r.get(0))
        .unwrap()
}

fn has_table(c: &Connection, name: &str) -> bool {
    c.query_row(
        "SELECT count(*) FROM sqlite_master WHERE type = 'table' AND name = ?1",
        [name],
        |r| r.get::<_, i64>(0),
    )
    .unwrap()
        > 0
}

#[test]
fn migration_from_v2_keeps_data_and_writes_the_backup() {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("lotse.db");
    {
        let c = Connection::open(&path).unwrap();
        c.execute_batch(V2_SCHEMA).unwrap();
        c.execute_batch(
            "INSERT INTO workspaces VALUES ('w1', 'Default', 0), ('w2', 'Client', 5);
             INSERT INTO projects VALUES ('p1','w2','praxis','/a','teal','folder',1,'{\"mode\":\"global\"}',1);
             INSERT INTO sessions VALUES ('s1','p1','t',1,'claude_code','claude',1,2);
             INSERT INTO transcripts VALUES ('/home/u/.claude/a.jsonl', 7);
             INSERT INTO settings VALUES ('other', 'kept');",
        )
        .unwrap();
        c.pragma_update(None, "user_version", 2u32).unwrap();
    }

    let db = Db::open(&path).unwrap();
    assert_eq!(db.schema_version().unwrap(), 3);
    assert_eq!(db.list_workspaces().unwrap().len(), 2, "workspaces survive");
    assert_eq!(db.list_projects().unwrap().len(), 1, "projects survive");
    assert_eq!(db.list_sessions().unwrap().len(), 1, "sessions survive");
    assert_eq!(
        db.all_transcripts().unwrap(),
        ["/home/u/.claude/a.jsonl"],
        "transcripts survive"
    );

    // the new tables work, and the old transcript row has no session yet
    let s = SessionId::new("s1");
    db.transition_span(&s, None, Status::Run, 10).unwrap();
    db.insert_perm_event(&s, "pr1", "Bash", "ls", 11).unwrap();
    db.tag_transcript("/home/u/.claude/a.jsonl", &s).unwrap();
    assert_eq!(db.list_spans(&s).unwrap().len(), 1);
    assert_eq!(db.list_perm_events(&s).unwrap().len(), 1);

    // the copy is the v2 database as it was: version 2, old rows, no new tables
    let bak = tmp.path().join("lotse.db.bak-v2");
    let b = Connection::open(&bak).unwrap();
    assert_eq!(user_version(&b), 2);
    assert!(!has_table(&b, "status_spans"));
    assert!(!has_table(&b, "perm_events"));
    let projects: i64 = b
        .query_row("SELECT count(*) FROM projects", [], |r| r.get(0))
        .unwrap();
    assert_eq!(projects, 1);
    use std::os::unix::fs::PermissionsExt;
    assert_eq!(
        std::fs::metadata(&bak).unwrap().permissions().mode() & 0o777,
        0o600,
        "the backup holds the same private data as the database"
    );

    // an up-to-date database is not backed up again
    drop(db);
    Db::open(&path).unwrap();
    assert!(!tmp.path().join("lotse.db.bak-v3").exists());
}

#[test]
fn a_fresh_database_needs_no_backup() {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("lotse.db");
    let db = Db::open(&path).unwrap();
    assert_eq!(db.schema_version().unwrap(), 3);
    let backups: Vec<_> = std::fs::read_dir(tmp.path())
        .unwrap()
        .filter_map(|e| e.ok())
        .filter(|e| e.file_name().to_string_lossy().contains(".bak-"))
        .collect();
    assert!(backups.is_empty(), "{backups:?}");
}

#[test]
fn status_spans_close_on_change() {
    let tmp = tempfile::tempdir().unwrap();
    let state = recording_state(Db::open_in_memory().unwrap(), tmp.path());
    let hub = &state.hub;
    let s = SessionId::new("s1");

    hub.broadcast(status_event("s1", Status::Run, 1_000));
    assert_eq!(
        lead(&state.db().list_spans(&s).unwrap()),
        [(Status::Run, 1_000, None)],
        "one open span"
    );

    hub.broadcast(perm_event("s1", "pr1", 5_000));
    hub.broadcast(status_event("s1", Status::Run, 9_000));
    hub.broadcast(status_event("s1", Status::Done, 20_000));
    assert_eq!(
        lead(&state.db().list_spans(&s).unwrap()),
        [
            (Status::Run, 1_000, Some(5_000)),
            (Status::Perm, 5_000, Some(9_000)),
            (Status::Run, 9_000, Some(20_000)),
            (Status::Done, 20_000, None),
        ]
    );

    // the same status again is no change: no new span, nothing closed
    hub.broadcast(status_event("s1", Status::Done, 25_000));
    assert_eq!(state.db().list_spans(&s).unwrap().len(), 4);

    // another session has its own lane
    hub.broadcast(status_event("s2", Status::Run, 30_000));
    assert_eq!(state.db().list_spans(&s).unwrap().len(), 4);

    // closing the session closes whatever is open, at the close
    hub.broadcast(Event::SessionClosed {
        session_id: s.clone(),
    });
    let spans = state.db().list_spans(&s).unwrap();
    assert!(
        spans.iter().all(|x| x.to_ms.is_some()),
        "a closed session keeps no open span: {spans:?}"
    );
    assert!(
        state
            .db()
            .list_spans(&SessionId::new("s2"))
            .unwrap()
            .iter()
            .all(|x| x.to_ms.is_none()),
        "the other session is untouched"
    );
}

#[test]
fn installing_twice_records_once() {
    // State::new will install the recorder itself; an install on top must not double anything
    let tmp = tempfile::tempdir().unwrap();
    let state = recording_state(Db::open_in_memory().unwrap(), tmp.path());
    history::install(&state.hub, state.db().clone());
    state.hub.broadcast(status_event("s1", Status::Run, 1_000));
    state.hub.broadcast(perm_event("s1", "pr1", 2_000));
    state.hub.broadcast(status_event("s1", Status::Run, 3_000));
    let s = SessionId::new("s1");
    assert_eq!(state.db().list_spans(&s).unwrap().len(), 3);
    assert_eq!(perm_rows(&state, "s1").len(), 1);
}

#[test]
fn a_late_event_never_makes_a_span_end_before_it_began() {
    let tmp = tempfile::tempdir().unwrap();
    let state = recording_state(Db::open_in_memory().unwrap(), tmp.path());
    state.hub.broadcast(status_event("s1", Status::Run, 5_000));
    state.hub.broadcast(status_event("s1", Status::Idle, 4_000));
    let spans = state.db().list_spans(&SessionId::new("s1")).unwrap();
    for sp in &spans {
        assert!(sp.to_ms.is_none_or(|to| to >= sp.from_ms), "{sp:?}");
    }
}

#[test]
fn sub_agents_get_their_own_lane() {
    let tmp = tempfile::tempdir().unwrap();
    let state = recording_state(Db::open_in_memory().unwrap(), tmp.path());
    let s = SessionId::new("s1");
    state.hub.broadcast(status_event("s1", Status::Run, 1_000));
    let sub = |id: &str, status| SubAgent {
        id: SubAgentId::new(id),
        name: id.into(),
        status,
    };
    state.hub.broadcast(Event::SubAgentsChanged {
        session_id: s.clone(),
        sub_agents: vec![sub("a1", Status::Run), sub("a2", Status::Run)],
    });
    state.hub.broadcast(Event::SubAgentStatus {
        session_id: s.clone(),
        sub_agent_id: SubAgentId::new("a1"),
        status: Status::Done,
    });
    // a2 left the list: its span closes
    state.hub.broadcast(Event::SubAgentsChanged {
        session_id: s.clone(),
        sub_agents: vec![sub("a1", Status::Done)],
    });
    let spans = state.db().list_spans(&s).unwrap();
    let of = |id: &str| -> Vec<(Status, bool)> {
        spans
            .iter()
            .filter(|x| x.sub_agent_id.as_deref() == Some(id))
            .map(|x| (x.status, x.to_ms.is_some()))
            .collect()
    };
    assert_eq!(of("a1"), [(Status::Run, true), (Status::Done, false)]);
    assert_eq!(of("a2"), [(Status::Run, true)]);
    assert_eq!(lead(&spans).len(), 1, "the lead lane is separate");
}

fn perm_rows(state: &State, s: &str) -> Vec<PermEventRow> {
    state.db().list_perm_events(&SessionId::new(s)).unwrap()
}

#[test]
fn perm_events_are_stamped_by_a_decision_or_by_the_terminal() {
    let tmp = tempfile::tempdir().unwrap();
    let state = recording_state(Db::open_in_memory().unwrap(), tmp.path());
    let hub = &state.hub;

    hub.broadcast(perm_event("s1", "pr1", 1_000));
    // the same prompt reported again is still one event
    hub.broadcast(perm_event("s1", "pr1", 1_500));
    let rows = perm_rows(&state, "s1");
    assert_eq!(rows.len(), 1);
    assert_eq!(
        (rows[0].tool.as_str(), rows[0].summary.as_str()),
        ("Bash", "ls")
    );
    assert_eq!(
        (rows[0].decided_at, rows[0].decision, rows[0].via),
        (None, None, None)
    );

    // answered in the terminal: the prompt goes away without a decision from Lotse
    hub.broadcast(status_event("s1", Status::Run, 4_000));
    let rows = perm_rows(&state, "s1");
    assert_eq!(rows[0].decided_at, Some(4_000));
    assert_eq!((rows[0].decision, rows[0].via), (None, None));

    // a decision names its prompt: a stale one stamps nothing, the current one does
    hub.broadcast(perm_event("s1", "pr2", 6_000));
    hub.publish_decision(&lotsed::hub::PermDecisionRecord {
        session_id: SessionId::new("s1"),
        prompt_id: Some("pr-older".into()),
        decision: PermDecision::Deny,
        via: Some(PermVia::Palette),
    });
    assert_eq!(perm_rows(&state, "s1")[1].decision, None);
    hub.publish_decision(&lotsed::hub::PermDecisionRecord {
        session_id: SessionId::new("s1"),
        prompt_id: Some("pr2".into()),
        decision: PermDecision::Always,
        via: Some(PermVia::Sidebar),
    });
    let rows = perm_rows(&state, "s1");
    assert_eq!(
        (rows[1].decision, rows[1].via),
        (Some(PermDecision::Always), Some(PermVia::Sidebar))
    );
    assert!(rows[1].decided_at.is_some_and(|at| at >= rows[1].asked_at));

    // the status change that follows the answer does not undo it
    hub.broadcast(status_event("s1", Status::Run, 9_000));
    let rows = perm_rows(&state, "s1");
    assert_eq!(rows[1].decision, Some(PermDecision::Always));
    assert_ne!(
        rows[1].decided_at,
        Some(9_000),
        "keeps the time of the answer"
    );
}

/// A pane that paints a permission prompt and waits for a key.
fn prompt_script(dir: &Path) -> String {
    std::fs::create_dir_all(dir).unwrap();
    common::script(
        dir,
        "agent.sh",
        "stty -icanon -echo min 1\n\
         printf ' Do you want to proceed?\\n ❯ 1. Yes\\n   2. No\\n'\n\
         dd bs=1 count=1 >/dev/null 2>&1\nsleep 5",
    );
    dir.join("agent.sh").to_string_lossy().into_owned()
}

#[test]
fn perm_event_records_decision_and_source() {
    let tmp = tempfile::tempdir().unwrap();
    let state = recording_state(Db::open_in_memory().unwrap(), tmp.path());
    let rt = tokio::runtime::Runtime::new().unwrap();
    let (client, _rx) = state.hub.register("test".into());
    let mut req_id = 0;
    let mut call = |request: Request| -> Response {
        req_id += 1;
        rt.block_on(state.handle(&client, req_id, request))
            .expect("a response")
    };

    let dir = tmp.path().join("proj");
    std::fs::create_dir_all(&dir).unwrap();
    let Response::Project(project) = call(Request::AddProject(NewProject {
        path: dir.to_string_lossy().into_owned(),
        name: None,
        color: None,
        icon: None,
    })) else {
        panic!("add_project")
    };
    let command = prompt_script(&tmp.path().join("script"));
    let Response::Session(s) = call(Request::CreateSession(NewSession {
        project_id: project.id,
        agent: AgentKind::ClaudeCode,
        command: Some(command),
        title: None,
        worktree: None,
        first_prompt: None,
    })) else {
        panic!("create_session")
    };
    let hooked = call(Request::HookEvent {
        agent: AgentKind::ClaudeCode,
        session_id: Some(s.id.clone()),
        payload: json!({"hook_event_name": "PermissionRequest", "tool_name": "Bash",
            "tool_input": {"command": "ls"}}),
    });
    assert!(matches!(hooked, Response::Ok), "{hooked:?}");

    let prompt_id = {
        let Response::Sessions(list) = call(Request::ListSessions { project_id: None }) else {
            panic!("list_sessions")
        };
        list.into_iter()
            .find(|x| x.id == s.id)
            .and_then(|x| x.perm)
            .and_then(|p| p.id)
            .expect("the open prompt has an id")
    };
    assert_eq!(
        perm_rows(&state, s.id.as_str()).len(),
        1,
        "asked, not yet decided"
    );
    assert_eq!(perm_rows(&state, s.id.as_str())[0].decision, None);

    // the prompt is read off the screen a moment after it is drawn: Conflict until then
    let start = Instant::now();
    loop {
        let r = call(Request::ResolvePermission {
            session_id: s.id.clone(),
            decision: PermDecision::Allow,
            via: Some(PermVia::Notification),
            prompt_id: Some(prompt_id.clone()),
        });
        if matches!(r, Response::Ok) {
            break;
        }
        assert!(
            start.elapsed() < Duration::from_secs(10),
            "never answerable: {r:?}"
        );
        std::thread::sleep(Duration::from_millis(50));
    }

    let rows = perm_rows(&state, s.id.as_str());
    assert_eq!(rows.len(), 1);
    let row = &rows[0];
    assert_eq!(row.prompt_id, prompt_id);
    assert_eq!(row.decision, Some(PermDecision::Allow));
    assert_eq!(row.via, Some(PermVia::Notification));
    assert!(row.decided_at.is_some_and(|at| at >= row.asked_at));
    state.kill_all_sessions();
}

#[test]
fn restart_keeps_history() {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("lotse.db");
    let s = SessionId::new("s1");
    let t0 = common_now() - 3_600_000; // an hour ago: inside the retention
    {
        let state = recording_state(Db::open(&path).unwrap(), tmp.path());
        state.hub.broadcast(status_event("s1", Status::Run, t0));
        state.hub.broadcast(perm_event("s1", "pr1", t0 + 2_000));
        // the daemon stops here: the perm span and the question are still open
    }

    let db = Db::open(&path).unwrap();
    assert_eq!(
        lead(&db.list_spans(&s).unwrap())[..1],
        [(Status::Run, t0, Some(t0 + 2_000))],
        "closed history is kept as it was"
    );
    assert_eq!(db.list_perm_events(&s).unwrap().len(), 1);

    let before = common_now();
    let state = recording_state(db, tmp.path());
    let spans = state.db().list_spans(&s).unwrap();
    assert_eq!(spans.len(), 2);
    let to = spans[1]
        .to_ms
        .expect("the open span was closed at the restart");
    assert!(to >= before, "closed at the restart, not at its start");
    assert_eq!(
        state.db().list_perm_events(&s).unwrap()[0].decided_at,
        Some(to),
        "an unanswered question ends with the daemon too"
    );
}

fn common_now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis() as u64
}

#[test]
fn history_older_than_thirty_days_is_dropped() {
    let tmp = tempfile::tempdir().unwrap();
    let db = Db::open_in_memory().unwrap();
    let old = SessionId::new("old");
    let new = SessionId::new("new");
    let now = common_now();
    let ancient = now - 31 * DAY_MS;
    db.transition_span(&old, None, Status::Run, ancient)
        .unwrap();
    db.transition_span(&old, None, Status::Done, ancient + 1_000)
        .unwrap();
    db.close_session_spans(&old, ancient + 2_000).unwrap();
    db.insert_perm_event(&old, "pr", "Bash", "ls", ancient)
        .unwrap();
    db.transition_span(&new, None, Status::Run, now - DAY_MS)
        .unwrap();
    db.insert_perm_event(&new, "pr", "Bash", "ls", now - DAY_MS)
        .unwrap();

    let _state = recording_state(db, tmp.path());
    let db = _state.db();
    assert!(db.list_spans(&old).unwrap().is_empty());
    assert!(db.list_perm_events(&old).unwrap().is_empty());
    assert_eq!(db.list_spans(&new).unwrap().len(), 1, "yesterday stays");
    assert_eq!(db.list_perm_events(&new).unwrap().len(), 1);
}

#[test]
fn a_new_session_opens_its_first_span_at_its_creation() {
    let tmp = tempfile::tempdir().unwrap();
    let state = recording_state(Db::open_in_memory().unwrap(), tmp.path());
    let session = Session {
        id: SessionId::new("s1"),
        project_id: lotse_proto::ProjectId::new("p1"),
        title: "t".into(),
        agent: AgentKind::ClaudeCode,
        command: "claude".into(),
        status: Status::Unknown,
        sub_agents: Vec::new(),
        git: Default::default(),
        created_at: 500,
        last_activity: 500,
        running_since: None,
        unread: false,
        perm: None,
        source: DetectSource::Process,
    };
    state.hub.broadcast(Event::SessionCreated(session));
    state.hub.broadcast(status_event("s1", Status::Run, 800));
    assert_eq!(
        lead(&state.db().list_spans(&SessionId::new("s1")).unwrap()),
        [(Status::Unknown, 500, Some(800)), (Status::Run, 800, None)]
    );
}

#[test]
fn the_daemon_state_records_history_without_a_manual_install() {
    let tmp = tempfile::tempdir().unwrap();
    let state = state_on(Db::open_in_memory().unwrap(), tmp.path());
    state.hub.broadcast(status_event("s1", Status::Run, 1_000));
    state.hub.broadcast(perm_event("s1", "pr1", 2_000));
    assert_eq!(
        state
            .db()
            .list_perm_events(&SessionId::new("s1"))
            .unwrap()
            .len(),
        1
    );
}
