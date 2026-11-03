//! Beta wp07, `get_transcript` per source: the Claude file with paired tool results and approvals
//! from `perm_events`, the newest Codex rollout of the project, and the ANSI-stripped scrollback of
//! agents without a transcript. Temp HOME and data dir: nothing of the real `~/.claude`, `~/.codex`
//! or Lotse data is read, and no agent runs (the panes are shell scripts).

mod common;

use common::{Client, Daemon};
use lotse_proto::{
    AgentKind, ErrorCode, NewProject, NewSession, PermDecision, PermVia, Project, Request,
    Response, Session, SessionId, ToolStatus, Transcript, TranscriptRole, TranscriptSource,
    TranscriptTurn,
};
use lotsed::config::Config;
use lotsed::db::Db;
use lotsed::hub::ClientTx;
use lotsed::state::State;
use serde_json::json;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, SystemTime};

const CLAUDE_TOOLS: &str = include_str!("fixtures/transcript/claude_tools.jsonl");
const CODEX_ROLLOUT: &str = include_str!("fixtures/transcript/codex_rollout.jsonl");

/// 2026-11-03T09:00:00Z
const T0: u64 = 1_793_696_400_000;

fn create(c: &mut Client, project: &Project, agent: AgentKind, command: &str) -> Session {
    match c.request(Request::CreateSession(NewSession {
        project_id: project.id.clone(),
        agent,
        command: Some(command.into()),
        title: None,
        worktree: None,
        first_prompt: None,
    })) {
        Response::Session(s) => s,
        other => panic!("create_session: {other:?}"),
    }
}

fn get(c: &mut Client, s: &Session, limit: Option<u32>) -> Transcript {
    match c.request(Request::GetTranscript {
        session_id: s.id.clone(),
        limit,
    }) {
        Response::Transcript(t) => t,
        other => panic!("get_transcript: {other:?}"),
    }
}

fn write(path: &Path, text: &str) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, text).unwrap();
}

fn set_mtime(path: &Path, secs_after_epoch: u64) {
    let file = std::fs::OpenOptions::new().write(true).open(path).unwrap();
    file.set_modified(SystemTime::UNIX_EPOCH + Duration::from_secs(secs_after_epoch))
        .unwrap();
}

fn rollout_for(cwd: &Path) -> String {
    CODEX_ROLLOUT.replace("@@CWD@@", &cwd.to_string_lossy())
}

/// role, text, and for a tool: name, summary, status, result
type Row = (
    TranscriptRole,
    String,
    Option<(String, String, Option<ToolStatus>, Option<String>)>,
);

fn rows(t: &Transcript) -> Vec<Row> {
    t.turns
        .iter()
        .map(|t| {
            (
                t.role,
                t.text.clone(),
                t.tool.as_ref().map(|x| {
                    (
                        x.name.clone(),
                        x.summary.clone(),
                        x.status,
                        x.result.clone(),
                    )
                }),
            )
        })
        .collect()
}

fn tool(
    name: &str,
    summary: &str,
    status: Option<ToolStatus>,
    result: Option<&str>,
) -> Option<(String, String, Option<ToolStatus>, Option<String>)> {
    Some((
        name.into(),
        summary.into(),
        status,
        result.map(str::to_owned),
    ))
}

#[test]
fn transcript_claude_pairs_tool_results_and_names_the_sub_agent() {
    let d = Daemon::start();
    let mut c = d.client();
    let project = c.add_project(&d.project_dir("proj"));
    let s = create(&mut c, &project, AgentKind::ClaudeCode, "/bin/sh");
    let file = d.home.join(".claude/projects/p/s.jsonl");
    write(&file, CLAUDE_TOOLS);
    c.expect_ok(Request::HookEvent {
        agent: AgentKind::ClaudeCode,
        session_id: Some(s.id.clone()),
        payload: json!({"hook_event_name": "UserPromptSubmit", "transcript_path": file}),
    });

    let t = get(&mut c, &s, None);
    assert_eq!(t.source, TranscriptSource::Claude);
    assert!(t.available && !t.truncated);
    use TranscriptRole::*;
    let text = |role, text: &str| (role, text.to_owned(), None);
    let want: Vec<Row> = vec![
        text(
            User,
            "Run the tests, then ask the explorer who calls round_cents",
        ),
        text(Assistant, "Running the tests."),
        (
            Tool,
            String::new(),
            tool(
                "Bash",
                "cargo test",
                Some(ToolStatus::Done),
                Some("test result: ok. 12 passed; 0 failed"),
            ),
        ),
        (
            Tool,
            String::new(),
            tool(
                "Task",
                "Find callers of round_cents",
                Some(ToolStatus::Fail),
                Some("explorer failed: boom"),
            ),
        ),
        text(Assistant, "Starting a long task."),
        // no result yet and nothing after it: still running
        (
            Tool,
            String::new(),
            tool("Bash", "sleep 600", Some(ToolStatus::Run), None),
        ),
    ];
    assert_eq!(rows(&t), want);
    let subs: Vec<_> = t.turns.iter().map(|t| t.sub_agent.as_deref()).collect();
    assert_eq!(
        subs,
        [None, None, None, Some("explorer"), None, None],
        "the Task call names its sub-agent"
    );
    assert_eq!(t.turns[0].at, Some(T0));
}

#[test]
fn transcript_codex_rollout() {
    let d = Daemon::start();
    let mut c = d.client();
    let dir = d.project_dir("proj");
    let project = c.add_project(&dir);
    let rollout = d
        .home
        .join(".codex/sessions/2026/11/03/rollout-2026-11-03T09-10-00-fixture.jsonl");
    write(&rollout, &rollout_for(&dir));
    let s = create(&mut c, &project, AgentKind::Codex, "/bin/sh");

    // no hook has fired: the newest rollout of this project is read from the Codex home
    let t = get(&mut c, &s, None);
    assert_eq!(t.source, TranscriptSource::Codex);
    assert!(t.available && !t.truncated);
    use TranscriptRole::*;
    let want: Vec<Row> = vec![
        (User, "add a unit test for round_cents".into(), None),
        (
            Tool,
            String::new(),
            tool(
                "shell",
                "cargo test round",
                Some(ToolStatus::Done),
                Some("test round ... ok"),
            ),
        ),
        (
            Tool,
            String::new(),
            tool(
                "shell",
                "cargo clippy",
                Some(ToolStatus::Fail),
                Some("error: lint failed"),
            ),
        ),
        (
            Assistant,
            "Added the test; clippy still fails.".into(),
            None,
        ),
    ];
    assert_eq!(
        rows(&t),
        want,
        "environment context and event_msg copies are dropped"
    );
    // the envelope's own timestamp
    assert_eq!(t.turns[0].at, Some(T0 + 10 * 60_000 + 2_000));
}

#[test]
fn transcript_codex_reads_the_newest_rollout_of_this_project_only() {
    let d = Daemon::start();
    let mut c = d.client();
    let (mine, other) = (d.project_dir("mine"), d.project_dir("other"));
    let project = c.add_project(&mine);
    let sessions = d.home.join(".codex/sessions/2026/11/03");
    let old_mine = sessions.join("rollout-a-old.jsonl");
    let new_mine = sessions.join("rollout-b-new.jsonl");
    let newest_other = sessions.join("rollout-c-other.jsonl");
    write(
        &old_mine,
        &rollout_for(&mine).replace("add a unit test", "OLD add a unit test"),
    );
    write(&new_mine, &rollout_for(&mine));
    write(&newest_other, &rollout_for(&other));
    set_mtime(&old_mine, 1_000);
    set_mtime(&new_mine, 2_000);
    set_mtime(&newest_other, 3_000);
    let s = create(&mut c, &project, AgentKind::Codex, "/bin/sh");

    let t = get(&mut c, &s, None);
    assert_eq!(t.turns[0].text, "add a unit test for round_cents");

    // a rollout of another project is never shown
    let lonely = d.project_dir("lonely");
    let project = c.add_project(&lonely);
    let s = create(&mut c, &project, AgentKind::Codex, "/bin/sh");
    let t = get(&mut c, &s, None);
    assert!(!t.available && t.turns.is_empty(), "{t:?}");
    assert_eq!(t.source, TranscriptSource::Codex);
}

#[test]
fn transcript_scrollback_fallback() {
    let d = Daemon::start();
    let mut c = d.client();
    let project = c.add_project(&d.project_dir("proj"));
    let dir = tempfile::tempdir().unwrap();
    common::script(
        dir.path(),
        "pane.sh",
        "printf 'build \\033[31mfailed\\033[0m\\n'\nprintf '\\033]0;a title\\007progress 10%%\\rprogress 100%%\\n'\nprintf 'line three\\n'\nsleep 5",
    );
    let script = dir.path().join("pane.sh").to_string_lossy().into_owned();
    for agent in [AgentKind::Terminal, AgentKind::OpenCode] {
        let s = create(&mut c, &project, agent, &script);
        c.attach(&s.id, 80, 24);
        assert!(c.wait_output(&s.id, "line three"), "{agent:?}");
        let t = get(&mut c, &s, None);
        assert_eq!(t.source, TranscriptSource::Scrollback, "{agent:?}");
        assert!(t.available);
        let text: Vec<_> = t.turns.iter().map(|t| t.text.as_str()).collect();
        assert_eq!(
            text.join("\n"),
            "build failed\nprogress 100%\nline three",
            "{agent:?}: colours, the window title and overwritten progress are gone"
        );
        assert!(t.turns.iter().all(|t| t.at.is_none() && t.tool.is_none()));
    }
}

#[test]
fn get_transcript_of_an_unknown_session_is_not_found() {
    let d = Daemon::start();
    let mut c = d.client();
    let r = c.request(Request::GetTranscript {
        session_id: SessionId::new("nope"),
        limit: None,
    });
    assert!(
        matches!(&r, Response::Error(e) if e.code == ErrorCode::NotFound),
        "{r:?}"
    );
}

#[test]
fn get_transcript_clamps_the_limit() {
    let d = Daemon::start();
    let mut c = d.client();
    let project = c.add_project(&d.project_dir("proj"));
    let s = create(&mut c, &project, AgentKind::ClaudeCode, "/bin/sh");
    let file = d.home.join(".claude/projects/p/many.jsonl");
    let lines: String = (0..250)
        .map(|i| {
            format!(
                "{{\"type\":\"user\",\"message\":{{\"role\":\"user\",\"content\":\"turn {i}\"}}}}\n"
            )
        })
        .collect();
    write(&file, &lines);
    c.expect_ok(Request::HookEvent {
        agent: AgentKind::ClaudeCode,
        session_id: Some(s.id.clone()),
        payload: json!({"hook_event_name": "UserPromptSubmit", "transcript_path": file}),
    });

    let t = get(&mut c, &s, Some(1_000));
    assert_eq!(t.turns.len(), 200, "more than 200 is held to 200");
    assert!(t.truncated);
    assert_eq!(t.turns.last().unwrap().text, "turn 249");
    assert_eq!(get(&mut c, &s, Some(0)).turns.len(), 1);
    assert_eq!(get(&mut c, &s, None).turns.len(), 50, "default");
}

#[test]
fn get_transcript_never_shows_secrets() {
    let d = Daemon::start();
    let mut c = d.client();
    let project = c.add_project(&d.project_dir("proj"));
    let s = create(&mut c, &project, AgentKind::ClaudeCode, "/bin/sh");
    let key = format!("sk-ant-api03-{}", "Zq7R".repeat(10));
    let pat = format!("ghp_{}", "a1B2c3D4e5".repeat(4));
    let file = d.home.join(".claude/projects/p/secret.jsonl");
    write(
        &file,
        &format!(
            "{{\"type\":\"user\",\"message\":{{\"role\":\"user\",\"content\":\"use {key} please\"}}}}\n\
             {{\"type\":\"assistant\",\"message\":{{\"role\":\"assistant\",\"content\":[{{\"type\":\"tool_use\",\"id\":\"t1\",\"name\":\"Bash\",\"input\":{{\"command\":\"export GITHUB_TOKEN={pat}\"}}}}]}}}}\n\
             {{\"type\":\"user\",\"message\":{{\"role\":\"user\",\"content\":[{{\"type\":\"tool_result\",\"tool_use_id\":\"t1\",\"content\":\"OPENAI_API_KEY=hunter2hunter2\"}}]}}}}\n"
        ),
    );
    c.expect_ok(Request::HookEvent {
        agent: AgentKind::ClaudeCode,
        session_id: Some(s.id.clone()),
        payload: json!({"hook_event_name": "UserPromptSubmit", "transcript_path": file}),
    });
    let t = get(&mut c, &s, None);
    let all = serde_json::to_string(&t).unwrap();
    for secret in [key.as_str(), pat.as_str(), "hunter2hunter2"] {
        assert!(!all.contains(secret), "{secret} reached the UI: {all}");
    }
    assert!(all.contains("[REDACTED]"));
    assert_eq!(t.turns[0].text, "use [REDACTED] please");
}

// ---- private keys cut by chunks, turns or the window ------------------------------------------------

/// A 4096-bit-shaped RSA key: 51 body lines of 65 base64 characters, each marked `KEYBODY<nn>` so a
/// test can tell that a line got through. Not a real key.
fn key_lines() -> Vec<String> {
    let mut lines = vec!["-----BEGIN RSA PRIVATE KEY-----".to_owned()];
    lines.extend((0..51).map(|i| format!("KEYBODY{i:02}{}", "aB3x".repeat(14))));
    lines.push("-----END RSA PRIVATE KEY-----".into());
    lines
}

/// 29 lines of output, the key (so it starts at line 30), then a last line.
fn scrollback_with_key() -> String {
    let mut lines: Vec<String> = (1..30).map(|i| format!("output line {i}")).collect();
    lines.extend(key_lines());
    lines.push("after the key".into());
    lines.join("\n") + "\n"
}

fn assert_no_key(text: &str, what: &str) {
    assert!(!text.contains("KEYBODY"), "{what}: a key line got through");
    assert!(
        !text.contains("END RSA PRIVATE KEY"),
        "{what}: the END line got through"
    );
}

#[test]
fn transcript_scrollback_never_shows_a_private_key_cut_by_chunks() {
    let d = Daemon::start();
    let mut c = d.client();
    let project = c.add_project(&d.project_dir("proj"));
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("out.txt"), scrollback_with_key()).unwrap();
    common::script(
        dir.path(),
        "pane.sh",
        &format!(
            "cat '{}'\nprintf 'all done\\n'\nsleep 5",
            dir.path().join("out.txt").display()
        ),
    );
    let script = dir.path().join("pane.sh").to_string_lossy().into_owned();
    for agent in [AgentKind::Terminal, AgentKind::OpenCode] {
        let s = create(&mut c, &project, agent, &script);
        c.attach(&s.id, 80, 24);
        assert!(c.wait_output(&s.id, "all done"), "{agent:?}");
        for limit in [None, Some(1), Some(2), Some(3), Some(200)] {
            let t = get(&mut c, &s, limit);
            let all = serde_json::to_string(&t).unwrap();
            assert_no_key(&all, &format!("{agent:?} limit {limit:?}"));
        }
        let all = get(&mut c, &s, None);
        let text: Vec<_> = all.turns.iter().map(|t| t.text.as_str()).collect();
        let text = text.join("\n");
        assert!(text.contains("output line 29\n[REDACTED]\nafter the key"));
        assert!(text.contains("output line 1\n"));
    }
}

/// `content` as one assistant line of a Claude transcript with these text blocks (each its own turn).
fn claude_blocks(blocks: &[String]) -> String {
    let content: Vec<_> = blocks
        .iter()
        .map(|b| json!({"type": "text", "text": b}))
        .collect();
    json!({"type": "assistant", "message": {"role": "assistant", "content": content}}).to_string()
        + "\n"
}

/// The key cut in the middle: the first block ends inside the body, the second holds the rest.
fn split_key() -> [String; 2] {
    let lines = key_lines();
    [
        format!("here is the key:\n{}", lines[..20].join("\n")),
        format!("{}\nthat was all", lines[20..].join("\n")),
    ]
}

#[test]
fn transcript_claude_never_shows_a_private_key_split_across_turns() {
    let d = Daemon::start();
    let mut c = d.client();
    let project = c.add_project(&d.project_dir("proj"));
    let s = create(&mut c, &project, AgentKind::ClaudeCode, "/bin/sh");
    let file = d.home.join(".claude/projects/p/split.jsonl");
    write(&file, &claude_blocks(&split_key()));
    c.expect_ok(Request::HookEvent {
        agent: AgentKind::ClaudeCode,
        session_id: Some(s.id.clone()),
        payload: json!({"hook_event_name": "UserPromptSubmit", "transcript_path": file}),
    });
    for limit in [None, Some(1), Some(2)] {
        let t = get(&mut c, &s, limit);
        assert!(t.available);
        assert_no_key(&serde_json::to_string(&t).unwrap(), &format!("{limit:?}"));
    }
    let t = get(&mut c, &s, Some(1));
    assert_eq!(
        t.turns[0].text, "[REDACTED]\nthat was all",
        "what follows the key stays"
    );
}

#[test]
fn transcript_codex_never_shows_a_private_key_split_across_turns() {
    let d = Daemon::start();
    let mut c = d.client();
    let dir = d.project_dir("proj");
    let project = c.add_project(&dir);
    let message = |text: &str| {
        json!({"timestamp": "2026-11-03T09:11:00.000Z", "type": "response_item", "payload":
            {"type": "message", "role": "assistant", "content": [{"type": "output_text", "text": text}]}})
        .to_string()
            + "\n"
    };
    let [first, second] = split_key();
    let rollout = d
        .home
        .join(".codex/sessions/2026/11/03/rollout-2026-11-03T09-10-00-split.jsonl");
    write(
        &rollout,
        &(rollout_for(&dir).lines().next().unwrap().to_owned()
            + "\n"
            + &message(&first)
            + &message(&second)),
    );
    let s = create(&mut c, &project, AgentKind::Codex, "/bin/sh");
    for limit in [None, Some(1), Some(2)] {
        let t = get(&mut c, &s, limit);
        assert!(t.available, "{limit:?}");
        assert_no_key(&serde_json::to_string(&t).unwrap(), &format!("{limit:?}"));
    }
}

// ---- which files a hook may name -----------------------------------------------------------------

#[test]
fn a_hook_cannot_name_a_jsonl_outside_the_agent_folders() {
    let d = Daemon::start();
    let mut c = d.client();
    let project = c.add_project(&d.project_dir("proj"));
    let s = create(&mut c, &project, AgentKind::ClaudeCode, "/bin/sh");
    let elsewhere = d.home.join("Documents/notes/s.jsonl");
    write(&elsewhere, CLAUDE_TOOLS);
    c.expect_ok(Request::HookEvent {
        agent: AgentKind::ClaudeCode,
        session_id: Some(s.id.clone()),
        payload: json!({"hook_event_name": "UserPromptSubmit", "transcript_path": elsewhere}),
    });
    let t = get(&mut c, &s, None);
    assert!(!t.available && t.turns.is_empty(), "{t:?}");
}

// ---- approvals, from `perm_events` of the in-process daemon ---------------------------------------

/// The daemon's `State` inside the test process (temp dirs, in-memory database): the test writes
/// `perm_events` rows with the times of the transcript fixture.
struct Local {
    state: Arc<State>,
    client: Arc<ClientTx>,
    rt: tokio::runtime::Runtime,
    tmp: tempfile::TempDir,
    next_req: u64,
}

impl Local {
    fn new() -> Self {
        let tmp = tempfile::tempdir().unwrap();
        let home = tmp.path().join("home");
        std::fs::create_dir_all(&home).unwrap();
        let mut cfg = Config::new(tmp.path().join("data"), false);
        cfg.home = Some(home);
        let state = State::new(cfg, Db::open_in_memory().unwrap()).unwrap();
        let (client, _rx) = state.hub.register("test".into());
        Self {
            state,
            client,
            rt: tokio::runtime::Runtime::new().unwrap(),
            tmp,
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

    fn claude_session(&mut self) -> Session {
        let dir = self.tmp.path().join("proj");
        std::fs::create_dir_all(&dir).unwrap();
        let Response::Project(project) = self.request(Request::AddProject(NewProject {
            path: dir.to_string_lossy().into_owned(),
            name: None,
            color: None,
            icon: None,
        })) else {
            panic!("add_project")
        };
        match self.request(Request::CreateSession(NewSession {
            project_id: project.id,
            agent: AgentKind::ClaudeCode,
            command: Some("/bin/sh".into()),
            title: None,
            worktree: None,
            first_prompt: None,
        })) {
            Response::Session(s) => s,
            other => panic!("create_session: {other:?}"),
        }
    }

    fn announce(&mut self, s: &Session, file: &PathBuf) {
        let r = self.request(Request::HookEvent {
            agent: AgentKind::ClaudeCode,
            session_id: Some(s.id.clone()),
            payload: json!({"hook_event_name": "UserPromptSubmit", "transcript_path": file}),
        });
        assert!(matches!(r, Response::Ok), "{r:?}");
    }

    fn transcript(&mut self, s: &Session) -> Transcript {
        match self.request(Request::GetTranscript {
            session_id: s.id.clone(),
            limit: None,
        }) {
            Response::Transcript(t) => t,
            other => panic!("get_transcript: {other:?}"),
        }
    }
}

impl Drop for Local {
    fn drop(&mut self) {
        self.state.kill_all_sessions();
    }
}

fn kinds(turns: &[TranscriptTurn]) -> Vec<String> {
    turns
        .iter()
        .map(|t| match (&t.role, &t.approval, &t.tool) {
            (TranscriptRole::Approval, Some(a), _) => {
                format!("approval {:?} {:?} {}", a.decision, a.via, a.command)
            }
            (role, _, Some(tool)) => format!("{role:?} {}", tool.name),
            (role, _, None) => format!("{role:?}"),
        })
        .collect()
}

#[test]
fn transcript_marks_approvals() {
    let mut l = Local::new();
    let s = l.claude_session();
    let file = l.tmp.path().join("home/.claude/projects/p/s.jsonl");
    write(&file, CLAUDE_TOOLS);
    l.announce(&s, &file);
    let db = l.state.db().clone();
    let at = |secs: u64| T0 + secs * 1_000;

    // answered from a notification at 09:00:15, between the Bash call and the Task call
    db.insert_perm_event(&s.id, "p1", "Bash", "npx playwright test", at(12))
        .unwrap();
    db.decide_perm_event(
        &s.id,
        Some("p1"),
        PermDecision::Allow,
        Some(PermVia::Notification),
        at(15),
    )
    .unwrap();
    // answered in the terminal: nobody knows what was decided, so it is not in the transcript
    db.insert_perm_event(&s.id, "p2", "Bash", "git push", at(20))
        .unwrap();
    db.end_perm_events(&s.id, at(25)).unwrap();
    // denied from the sidebar at 09:00:45, before the assistant's 09:00:50 turn
    db.insert_perm_event(&s.id, "p3", "Bash", "rm -rf build", at(44))
        .unwrap();
    db.decide_perm_event(
        &s.id,
        Some("p3"),
        PermDecision::Deny,
        Some(PermVia::Sidebar),
        at(45),
    )
    .unwrap();
    // asked and never answered (the daemon stopped): not a decision either
    db.insert_perm_event(&s.id, "p4", "Bash", "later", at(49))
        .unwrap();

    let t = l.transcript(&s);
    assert_eq!(
        kinds(&t.turns),
        [
            "User",
            "Assistant",
            "Tool Bash",
            "approval Allow Some(Notification) Bash(npx playwright test)",
            "Tool Task",
            "approval Deny Some(Sidebar) Bash(rm -rf build)",
            "Assistant",
            "Tool Bash",
        ]
    );
    let approval = &t.turns[3];
    assert_eq!(approval.at, Some(at(15)), "stamped when decided");
    assert!(approval.text.is_empty() && approval.tool.is_none());
}

#[test]
fn transcript_approvals_older_than_the_window_are_left_out() {
    let mut l = Local::new();
    let s = l.claude_session();
    let file = l.tmp.path().join("home/.claude/projects/p/s.jsonl");
    write(&file, CLAUDE_TOOLS);
    l.announce(&s, &file);
    l.state
        .db()
        .insert_perm_event(&s.id, "p1", "Bash", "ls", T0 + 12_000)
        .unwrap();
    l.state
        .db()
        .decide_perm_event(
            &s.id,
            Some("p1"),
            PermDecision::Always,
            Some(PermVia::Palette),
            T0 + 15_000,
        )
        .unwrap();
    // the newest two turns only: the approval at 09:00:15 is older than both
    let Response::Transcript(t) = l.request(Request::GetTranscript {
        session_id: s.id.clone(),
        limit: Some(2),
    }) else {
        panic!("get_transcript")
    };
    assert_eq!(kinds(&t.turns), ["Assistant", "Tool Bash"]);
    assert!(t.truncated);
}
