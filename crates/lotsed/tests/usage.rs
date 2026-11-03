//! Beta wp06: `get_usage` (time, tokens, spend of one session) on an in-process `State` with the
//! recorder. Transcript fixtures live in a temp HOME: never the real `~/.claude`.
//!
//! What depends on the day (today's spend) runs through `usage_at` with a pinned clock: noon of the
//! local day, never near a midnight whatever the time zone or the time the suite runs.

use lotse_proto::{
    AgentKind, ErrorCode, LaneKind, NewProject, NewSession, Project, Request, Response, Session,
    SessionId, Status, Usage,
};
use lotsed::config::Config;
use lotsed::db::Db;
use lotsed::session_usage::usage_at;
use lotsed::state::State;
use lotsed::usage::DayBounds;
use serde_json::json;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

const SECOND: u64 = 1000;
const DAY_MS: u64 = 86_400_000;

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_millis() as u64
}

/// `2026-11-02T10:00:00.000Z` (civil-from-days, Howard Hinnant).
fn iso(ms: u64) -> String {
    let days = (ms / DAY_MS) as i64;
    let rem = ms % DAY_MS;
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    format!(
        "{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}.{:03}Z",
        rem / 3_600_000,
        rem / 60_000 % 60,
        rem / 1000 % 60,
        rem % 1000
    )
}

fn line(id: &str, model: &str, at: u64, usage: (u64, u64, u64, u64)) -> String {
    format!(
        "{{\"type\":\"assistant\",\"timestamp\":\"{}\",\"requestId\":\"r-{id}\",\"message\":{{\"id\":\"{id}\",\"model\":\"{model}\",\"role\":\"assistant\",\"content\":[],\"usage\":{{\"input_tokens\":{},\"output_tokens\":{},\"cache_read_input_tokens\":{},\"cache_creation_input_tokens\":{}}}}}}}\n",
        iso(at),
        usage.0,
        usage.1,
        usage.2,
        usage.3
    )
}

/// Noon of the local day the suite runs on: the pinned "now". Fixtures hang off it, so nothing
/// crosses the day boundary when the suite runs just after midnight.
fn noon() -> u64 {
    DayBounds::local(now_ms()).start_ms + 12 * 3_600_000
}

/// Set the file's modification time (a transcript last written before today is not read).
fn stamp(path: &Path, at: u64) {
    let f = std::fs::OpenOptions::new().write(true).open(path).unwrap();
    f.set_modified(UNIX_EPOCH + Duration::from_millis(at))
        .unwrap();
}

struct Local {
    state: Arc<State>,
    client: Arc<lotsed::hub::ClientTx>,
    rt: tokio::runtime::Runtime,
    tmp: tempfile::TempDir,
    home: PathBuf,
    next_req: u64,
}

impl Local {
    fn new() -> Self {
        let tmp = tempfile::tempdir().unwrap();
        let home = tmp.path().join("home");
        std::fs::create_dir_all(&home).unwrap();
        let mut cfg = Config::new(tmp.path().join("data"), false);
        cfg.home = Some(home.clone());
        let state = State::new(cfg, Db::open_in_memory().unwrap()).unwrap();
        let (client, _rx) = state.hub.register("test".into());
        Self {
            state,
            client,
            rt: tokio::runtime::Runtime::new().unwrap(),
            tmp,
            home,
            next_req: 0,
        }
    }

    fn request(&mut self, request: Request) -> Response {
        self.next_req += 1;
        let (state, client, id) = (&self.state, &self.client, self.next_req);
        self.rt
            .block_on(state.handle(client, id, request))
            .expect("a response")
    }

    fn project(&mut self, name: &str) -> Project {
        let dir = self.tmp.path().join(name);
        std::fs::create_dir_all(&dir).unwrap();
        match self.request(Request::AddProject(NewProject {
            path: dir.to_string_lossy().into_owned(),
            name: None,
            color: None,
            icon: None,
        })) {
            Response::Project(p) => p,
            other => panic!("add_project: {other:?}"),
        }
    }

    fn session(&mut self, project: &Project, agent: AgentKind) -> Session {
        match self.request(Request::CreateSession(NewSession {
            project_id: project.id.clone(),
            agent,
            command: Some("/bin/sh".into()),
            title: None,
            worktree: None,
            first_prompt: None,
        })) {
            Response::Session(s) => s,
            other => panic!("create_session: {other:?}"),
        }
    }

    /// A transcript file under the temp HOME, last written at `written` (the pinned clock).
    fn transcript(&self, name: &str, text: &str, written: u64) -> PathBuf {
        let dir = self.home.join(".claude/projects/p");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join(name);
        std::fs::write(&path, text).unwrap();
        stamp(&path, written);
        // the daemon keeps the resolved path (/var is /private/var on macOS)
        std::fs::canonicalize(path).unwrap()
    }

    fn hook(&mut self, s: &Session, payload: serde_json::Value) {
        let r = self.request(Request::HookEvent {
            agent: s.agent,
            session_id: Some(s.id.clone()),
            payload,
        });
        assert!(matches!(r, Response::Ok), "{r:?}");
    }

    /// A Claude Code session whose hooks announced `transcript`.
    fn claude_with(&mut self, project: &Project, transcript: &Path) -> Session {
        let s = self.session(project, AgentKind::ClaudeCode);
        self.hook(
            &s,
            json!({"hook_event_name": "SessionStart", "transcript_path": transcript}),
        );
        s
    }

    fn usage(&mut self, id: &SessionId) -> Usage {
        match self.request(Request::GetUsage {
            session_id: id.clone(),
        }) {
            Response::Usage(u) => u,
            other => panic!("get_usage: {other:?}"),
        }
    }

    /// Poll until `done` holds (the clock moves the open spans on).
    fn usage_until(&mut self, id: &SessionId, done: impl Fn(&Usage) -> bool) -> Usage {
        let start = Instant::now();
        loop {
            let u = self.usage(id);
            if done(&u) {
                return u;
            }
            assert!(start.elapsed() < Duration::from_secs(10), "never: {u:?}");
            std::thread::sleep(Duration::from_millis(10));
        }
    }
}

impl Drop for Local {
    fn drop(&mut self) {
        self.state.kill_all_sessions();
    }
}

#[test]
fn usage_splits_tokens_and_time() {
    let mut l = Local::new();
    let now = noon();
    let (a, b) = (l.project("praxis-web"), l.project("other"));

    // Opus 4.5: $5 in, $25 out, cache read $0.50, cache write $6.25 per million = $11.00; Sonnet 4.5
    // $3 in, $15 out. The first message is written twice (one line per content block): counted once.
    let mut text = line(
        "m0",
        "claude-sonnet-4-5",
        now - 2 * DAY_MS,
        (100, 0, 0, 0), // before today: in the tokens, not in today's spend
    );
    let m1 = line(
        "m1",
        "claude-opus-4-5-20251101",
        now - 60 * SECOND,
        (1_000_000, 100_000, 2_000_000, 400_000),
    );
    text += &m1;
    text += &m1;
    text += &line(
        "m2",
        "claude-sonnet-4-5",
        now - 30 * SECOND,
        (1000, 500, 0, 0),
    );
    let mine = l.transcript("mine.jsonl", &text, now);
    l.claude_with(&a, &mine);

    // a second session of the same project, one of another project
    let sibling = l.transcript(
        "sibling.jsonl",
        &line(
            "s1",
            "claude-sonnet-4-5",
            now - 10 * SECOND,
            (1000, 0, 0, 0),
        ),
        now,
    );
    l.claude_with(&a, &sibling);
    let stranger = l.transcript(
        "stranger.jsonl",
        &line("t1", "claude-haiku-4-5", now - 10 * SECOND, (0, 1000, 0, 0)),
        now,
    );
    l.claude_with(&b, &stranger);
    l.state.refresh_usage_at(now); // the status bar's total, as the 5 s tick makes it

    // The session asked about: ten minutes old, 3 min running, 3 min waiting for a permission, then
    // running again until `now`. Its history is recorded from events at pinned times.
    let mut s = l.session(&a, AgentKind::ClaudeCode);
    s.id = SessionId::new("pinned");
    s.created_at = now - 600 * SECOND;
    let ev = |status, at, perm| lotse_proto::Event::StatusChanged {
        session_id: s.id.clone(),
        status,
        source: lotse_proto::DetectSource::Hook,
        perm,
        running_since: None,
        at,
    };
    let ask = lotse_proto::PermRequest {
        tool: "Bash".into(),
        summary: "ls".into(),
        always: None,
        id: Some("pr1".into()),
    };
    l.state
        .hub
        .broadcast(ev(Status::Run, now - 540 * SECOND, None));
    l.state
        .hub
        .broadcast(ev(Status::Perm, now - 360 * SECOND, Some(ask)));
    l.state
        .hub
        .broadcast(ev(Status::Run, now - 180 * SECOND, None));

    let u = usage_at(&l.state, &s, Some(&mine), now);
    assert_eq!(
        (
            u.tokens.input,
            u.tokens.cache_read,
            u.tokens.cache_write,
            u.tokens.output
        ),
        (1_001_100, 2_000_000, 400_000, 100_500),
        "the four kinds, over the whole transcript, each message once"
    );
    assert!(u.priced);
    assert_eq!(
        u.model.as_deref(),
        Some("claude-sonnet-4-5"),
        "the latest message's"
    );
    // today: 11.00 + 0.0105; the lane carries the whole session (m0 too: $0.0003)
    assert_eq!(u.spend.session, 11_010_500);
    assert_eq!(u.spend.project, 11_010_500 + 3_000);
    assert_eq!(u.spend.all, 11_010_500 + 3_000 + 5_000);
    assert_eq!(u.lanes[0].cost_micros, 11_010_800);

    // the panel's all-projects spend is the status bar's total, not a second count
    let Response::Snapshot(snapshot) = l.request(Request::GetSnapshot) else {
        panic!("get_snapshot")
    };
    assert_eq!(snapshot.usage.cost_micros, Some(u.spend.all));

    // time: `run` is working, `perm` is waiting; the open span runs to now
    assert_eq!(u.started_at, s.created_at);
    assert_eq!(u.total_ms, 600 * SECOND);
    assert_eq!(u.working_ms, (180 + 180) * SECOND);
    assert_eq!(u.waiting_ms, 180 * SECOND);
    let lane = &u.lanes[0];
    assert_eq!(
        (lane.name.as_str(), lane.kind),
        ("Claude Code", LaneKind::Lead)
    );
    let shape: Vec<_> = lane
        .spans
        .iter()
        .map(|x| (x.status, x.from, x.to))
        .collect();
    assert_eq!(
        shape,
        [
            (Status::Run, now - 540 * SECOND, now - 360 * SECOND),
            (Status::Perm, now - 360 * SECOND, now - 180 * SECOND),
            (Status::Run, now - 180 * SECOND, now),
        ]
    );
}

#[test]
fn time_follows_the_hooks_of_a_live_session() {
    let mut l = Local::new();
    let p = l.project("praxis-web");
    let s = l.session(&p, AgentKind::ClaudeCode);
    l.hook(&s, json!({"hook_event_name": "UserPromptSubmit"}));
    let u = l.usage_until(&s.id, |u| u.working_ms >= 30);
    assert_eq!(u.started_at, s.created_at);
    assert_eq!(u.waiting_ms, 0);
    l.hook(
        &s,
        json!({"hook_event_name": "PermissionRequest", "tool_name": "Bash",
            "tool_input": {"command": "ls"}}),
    );
    let u = l.usage_until(&s.id, |u| u.waiting_ms >= 30);
    assert!(u.working_ms + u.waiting_ms <= u.total_ms, "{u:?}");
    let working = u.working_ms;
    assert_eq!(l.usage(&s.id).working_ms, working, "the run span is closed");
    let statuses: Vec<_> = u.lanes[0].spans.iter().map(|x| x.status).collect();
    assert_eq!(statuses[statuses.len() - 2..], [Status::Run, Status::Perm]);
}

#[test]
fn a_repeated_get_usage_does_not_read_unchanged_transcripts_again() {
    let mut l = Local::new();
    let now = now_ms();
    let (a, b) = (l.project("praxis-web"), l.project("other"));
    let mine = l.transcript(
        "mine.jsonl",
        &line("m1", "claude-sonnet-4-5", now - SECOND, (1000, 0, 0, 0)),
        now,
    );
    let s = l.claude_with(&a, &mine);
    let sibling = l.transcript(
        "sibling.jsonl",
        &line("s1", "claude-sonnet-4-5", now - SECOND, (10, 0, 0, 0)),
        now,
    );
    l.claude_with(&a, &sibling);
    let stranger = l.transcript(
        "stranger.jsonl",
        &line("t1", "claude-haiku-4-5", now - SECOND, (0, 10, 0, 0)),
        now,
    );
    l.claude_with(&b, &stranger);

    let start = l.state.usage_scan_reads();
    let first = l.usage(&s.id);
    assert_eq!(first.tokens.input, 1000);
    let reads = l.state.usage_scan_reads() - start;
    assert_eq!(
        reads, 2,
        "this session and its sibling; the other project comes from the status bar's tracker"
    );

    let second = l.usage(&s.id);
    assert_eq!(l.state.usage_scan_reads() - start, reads, "nothing re-read");
    assert_eq!(second.tokens, first.tokens);
    assert_eq!(second.spend, first.spend);

    // a changed file is read again, only that one
    let mut text = std::fs::read_to_string(&mine).unwrap();
    text += &line("m2", "claude-sonnet-4-5", now, (500, 0, 0, 0));
    std::fs::write(&mine, text).unwrap();
    let third = l.usage(&s.id);
    assert_eq!(third.tokens.input, 1500);
    assert_eq!(l.state.usage_scan_reads() - start, reads + 1);
}

#[test]
fn an_unknown_model_leaves_the_price_open() {
    let mut l = Local::new();
    let now = noon();
    let a = l.project("praxis-web");
    let mut text = line("a", "claude-sonnet-4-5", now - SECOND, (1_000_000, 0, 0, 0));
    text += &line("b", "claude-fable-9-9", now - SECOND, (500, 0, 0, 0));
    let path = l.transcript("mixed.jsonl", &text, now);
    let s = l.claude_with(&a, &path);
    l.state.refresh_usage_at(now);
    let u = usage_at(&l.state, &s, Some(&path), now);
    assert!(!u.priced);
    assert_eq!(u.tokens.input, 1_000_500, "the tokens are all counted");
    assert_eq!(
        u.spend.session, 3_000_000,
        "the spend covers the priced part"
    );
    assert_eq!(u.spend.all, 3_000_000, "the status bar's priced part, too");
}

#[test]
fn unknown_session_is_not_found() {
    let mut l = Local::new();
    let r = l.request(Request::GetUsage {
        session_id: SessionId::new("nope"),
    });
    assert!(
        matches!(&r, Response::Error(e) if e.code == ErrorCode::NotFound),
        "{r:?}"
    );
}

#[test]
fn a_codex_session_without_a_transcript_has_time_but_no_tokens() {
    let mut l = Local::new();
    let p = l.project("praxis-web");
    let s = l.session(&p, AgentKind::Codex);
    let u = l.usage_until(&s.id, |u| u.total_ms >= 20);
    assert_eq!(u.tokens, Default::default());
    assert!(!u.priced);
    assert_eq!(u.spend.session, 0);
    assert_eq!(u.started_at, s.created_at);
    assert_eq!(u.lanes.len(), 1, "the agent's own lane");
    assert_eq!(u.lanes[0].kind, LaneKind::Lead);
    assert!(!u.lanes[0].spans.is_empty(), "its time is on the timeline");
}

#[test]
fn a_transcript_outside_the_home_folder_is_not_read() {
    let mut l = Local::new();
    let p = l.project("praxis-web");
    let outside = l.tmp.path().join("elsewhere.jsonl");
    std::fs::write(
        &outside,
        line("a", "claude-sonnet-4-5", now_ms() - SECOND, (1000, 0, 0, 0)),
    )
    .unwrap();
    let s = l.claude_with(&p, &outside);
    let u = l.usage(&s.id);
    assert_eq!(u.tokens, Default::default());
    assert!(!u.priced);
}

#[test]
fn a_sub_agent_has_its_own_lane() {
    let mut l = Local::new();
    let p = l.project("praxis-web");
    let s = l.session(&p, AgentKind::ClaudeCode);
    l.state.hub.broadcast(lotse_proto::Event::SubAgentsChanged {
        session_id: s.id.clone(),
        sub_agents: vec![lotse_proto::SubAgent {
            id: lotse_proto::SubAgentId::new("a1"),
            name: "Explore".into(),
            status: Status::Run,
        }],
    });
    let u = l.usage(&s.id);
    let kinds: Vec<_> = u.lanes.iter().map(|x| (x.kind, x.name.as_str())).collect();
    assert_eq!(
        kinds,
        [(LaneKind::Lead, "Claude Code"), (LaneKind::SubAgent, "a1")],
        "named by id: the daemon keeps no sub-agent names"
    );
}
