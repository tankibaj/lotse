//! Beta wp07, `export_transcript` and the redaction behind it. Token-shaped values are built at run
//! time from pieces (none of them is a real credential, and no secret scanner trips on the file).
//! Temp HOME and data dir: nothing of the real `~/.claude` or Lotse data is read.

mod common;

use common::{Client, Daemon};
use lotse_proto::{
    AgentKind, ErrorCode, Export, ExportFormat, Message, NewSession, Project, Request,
    RequestEnvelope, Response, Session, SessionId, TranscriptTurn,
};
use lotsed::redact::{MASK, redact};
use serde_json::json;
use std::path::Path;

const CLAUDE_TOOLS: &str = include_str!("fixtures/transcript/claude_tools.jsonl");
const CLAUDE_SECRETS: &str = include_str!("fixtures/transcript/claude_secrets.jsonl");

struct Secrets {
    anthropic: String,
    bearer: String,
    url_password: String,
    github: String,
    password: String,
    aws: String,
    private_key: String,
    jwt: String,
    slack: String,
    gitlab: String,
}

fn secrets() -> Secrets {
    Secrets {
        anthropic: format!("sk-ant-api03-{}", "Zq7R".repeat(10)),
        bearer: "a1b2c3d4e5f6g7h8i9j0".into(),
        url_password: "hunter2-pa55word".into(),
        github: format!("ghp_{}", "a1B2c3D4e5".repeat(4)),
        password: "Tr0ub4dor&3xyz".into(),
        aws: format!("AKIA{}", "IOSFODNN7EXAMPLE"),
        private_key: [
            "-----BEGIN RSA PRIVATE KEY-----",
            "MIIEowIBAAKCAQEA7notarealkey",
            "Zm9vYmFyYmF6",
            "-----END RSA PRIVATE KEY-----",
        ]
        .join("\n"),
        jwt: [
            "eyJhbGciOiJIUzI1NiJ9",
            "eyJzdWIiOiJmaXh0dXJlIn0",
            "c2lnbmF0dXJlLWZpeHR1cmU",
        ]
        .join("."),
        slack: format!("xoxb-{}", "123456789012-abcdefghijklmnop"),
        gitlab: format!("glpat-{}", "abcdefghij0123456789"),
    }
}

impl Secrets {
    fn all(&self) -> Vec<&str> {
        vec![
            &self.anthropic,
            &self.bearer,
            &self.url_password,
            &self.github,
            &self.password,
            &self.aws,
            &self.jwt,
            &self.slack,
            &self.gitlab,
            "MIIEowIBAAKCAQEA7notarealkey",
            "Zm9vYmFyYmF6",
        ]
    }

    /// The fixture with the placeholders filled (JSON-escaped: the private key has newlines).
    fn fill(&self, fixture: &str) -> String {
        let json = |s: &str| {
            let quoted = serde_json::to_string(s).unwrap();
            quoted[1..quoted.len() - 1].to_owned()
        };
        fixture
            .replace("@@ANTHROPIC@@", &json(&self.anthropic))
            .replace("@@BEARER@@", &json(&self.bearer))
            .replace("@@URLPASS@@", &json(&self.url_password))
            .replace("@@GITHUB@@", &json(&self.github))
            .replace("@@PASSWORD@@", &json(&self.password))
            .replace("@@AWS@@", &json(&self.aws))
            .replace("@@PRIVATE_KEY@@", &json(&self.private_key))
            .replace("@@JWT@@", &json(&self.jwt))
            .replace("@@SLACK@@", &json(&self.slack))
            .replace("@@GITLAB@@", &json(&self.gitlab))
    }
}

fn claude_session(c: &mut Client, d: &Daemon, fixture: &str) -> (Project, Session) {
    let project = c.add_project(&d.project_dir("proj"));
    let s = match c.request(Request::CreateSession(NewSession {
        project_id: project.id.clone(),
        agent: AgentKind::ClaudeCode,
        command: Some("/bin/sh".into()),
        title: Some("Round cents".into()),
        worktree: None,
        first_prompt: None,
        parent_session: None,
        requested_by_agent: false,
    })) {
        Response::Session(s) => s,
        other => panic!("create_session: {other:?}"),
    };
    let file = d.home.join(".claude/projects/p/s.jsonl");
    std::fs::create_dir_all(file.parent().unwrap()).unwrap();
    std::fs::write(&file, fixture).unwrap();
    announce(c, &s, &file);
    (project, s)
}

fn announce(c: &mut Client, s: &Session, file: &Path) {
    c.expect_ok(Request::HookEvent {
        agent: AgentKind::ClaudeCode,
        session_id: Some(s.id.clone()),
        payload: json!({"hook_event_name": "UserPromptSubmit", "transcript_path": file}),
    });
}

fn export(c: &mut Client, s: &Session, format: ExportFormat) -> Export {
    match c.request(Request::ExportTranscript {
        session_id: s.id.clone(),
        format,
    }) {
        Response::Export(e) => e,
        other => panic!("export_transcript: {other:?}"),
    }
}

const FORMATS: [ExportFormat; 3] = [
    ExportFormat::Markdown,
    ExportFormat::JsonLines,
    ExportFormat::Text,
];

// ---- the document ------------------------------------------------------------------------------

#[test]
fn export_formats_and_file_names() {
    let d = Daemon::start();
    let mut c = d.client();
    let (_, s) = claude_session(&mut c, &d, CLAUDE_TOOLS);

    let md = export(&mut c, &s, ExportFormat::Markdown);
    let re = regex::Regex::new(r"^proj-Round-cents-\d{8}-\d{4}\.md$").unwrap();
    assert!(re.is_match(&md.file_name), "{}", md.file_name);
    for needle in [
        "proj",
        "Round cents",
        "claude-opus-4-5",
        "2026-11-03 09:00:00Z",
        "Run the tests, then ask the explorer who calls round_cents",
        "cargo test",
        "test result: ok. 12 passed; 0 failed",
        "explorer failed: boom",
    ] {
        assert!(
            md.content.contains(needle),
            "{needle} missing:\n{}",
            md.content
        );
    }
    assert_eq!(md.redactions, 0);

    let jl = export(&mut c, &s, ExportFormat::JsonLines);
    assert!(jl.file_name.ends_with(".jsonl"), "{}", jl.file_name);
    let turns: Vec<TranscriptTurn> = jl
        .content
        .lines()
        .map(|l| serde_json::from_str(l).expect("one turn per line"))
        .collect();
    assert_eq!(turns.len(), 6);
    assert_eq!(turns[3].sub_agent.as_deref(), Some("explorer"));

    let txt = export(&mut c, &s, ExportFormat::Text);
    assert!(txt.file_name.ends_with(".txt"), "{}", txt.file_name);
    assert_eq!(
        txt.content,
        "[2026-11-03 09:00:00Z] user: Run the tests, then ask the explorer who calls round_cents\n\
         [2026-11-03 09:00:05Z] assistant: Running the tests.\n\
         [2026-11-03 09:00:05Z] tool Bash (done): cargo test\n\
         \x20 > test result: ok. 12 passed; 0 failed\n\
         [2026-11-03 09:00:30Z] tool Task (fail): Find callers of round_cents [sub-agent explorer]\n\
         \x20 > explorer failed: boom\n\
         [2026-11-03 09:00:50Z] assistant: Starting a long task.\n\
         [2026-11-03 09:00:50Z] tool Bash (run): sleep 600\n",
        "zero secrets: nothing masked, nothing else changed"
    );
    assert_eq!(txt.redactions, 0);
}

#[test]
fn export_redacts_secrets() {
    let d = Daemon::start();
    let mut c = d.client();
    let sec = secrets();
    let (_, s) = claude_session(&mut c, &d, &sec.fill(CLAUDE_SECRETS));

    for format in FORMATS {
        let e = export(&mut c, &s, format);
        for secret in sec.all() {
            assert!(
                !e.content.contains(secret),
                "{format:?}: {secret} is in the export:\n{}",
                e.content
            );
        }
        assert!(!e.content.contains("BEGIN RSA PRIVATE KEY"), "{format:?}");
        assert!(e.content.contains(MASK), "{format:?}");
        assert!(
            e.content.contains("Deployed. Nothing else to report."),
            "{format:?}: what is not secret stays"
        );
        // the user's sentence, the tool input and the tool's output are all masked
        assert!(
            e.content.contains("ANTHROPIC_API_KEY=[REDACTED]"),
            "{format:?}:\n{}",
            e.content
        );
        // 2 in the prompt, 2 in the assistant turn (URL password, GITHUB_TOKEN), 6 in the output
        assert_eq!(e.redactions, 10, "{format:?}");
    }
}

/// A 4096-bit-shaped RSA key at line 30 of a terminal's output (51 body lines, each `KEYBODY<nn>`):
/// the chunks of the scrollback cut it, the export must still hold none of it.
#[test]
fn export_of_a_scrollback_never_holds_a_private_key_cut_by_chunks() {
    let d = Daemon::start();
    let mut c = d.client();
    let project = c.add_project(&d.project_dir("proj"));
    let mut lines: Vec<String> = (1..30).map(|i| format!("output line {i}")).collect();
    lines.push("-----BEGIN RSA PRIVATE KEY-----".into());
    lines.extend((0..51).map(|i| format!("KEYBODY{i:02}{}", "aB3x".repeat(14))));
    lines.push("-----END RSA PRIVATE KEY-----".into());
    lines.push("after the key".into());
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("out.txt"), lines.join("\n") + "\n").unwrap();
    common::script(
        dir.path(),
        "pane.sh",
        &format!(
            "cat '{}'\nprintf 'all done\\n'\nsleep 5",
            dir.path().join("out.txt").display()
        ),
    );
    let s = match c.request(Request::CreateSession(NewSession {
        project_id: project.id,
        agent: AgentKind::Terminal,
        command: Some(dir.path().join("pane.sh").to_string_lossy().into_owned()),
        title: None,
        worktree: None,
        first_prompt: None,
        parent_session: None,
        requested_by_agent: false,
    })) {
        Response::Session(s) => s,
        other => panic!("create_session: {other:?}"),
    };
    c.attach(&s.id, 80, 24);
    assert!(c.wait_output(&s.id, "all done"));
    for format in FORMATS {
        let e = export(&mut c, &s, format);
        assert!(!e.content.contains("KEYBODY"), "{format:?}:\n{}", e.content);
        assert!(
            !e.content.contains("END RSA PRIVATE KEY"),
            "{format:?}:\n{}",
            e.content
        );
        assert!(e.content.contains("after the key"), "{format:?}");
        assert!(e.redactions >= 1, "{format:?}: the key was counted");
    }
}

#[test]
fn export_of_an_unknown_session_is_not_found() {
    let d = Daemon::start();
    let mut c = d.client();
    let r = c.request(Request::ExportTranscript {
        session_id: SessionId::new("nope"),
        format: ExportFormat::Text,
    });
    assert!(
        matches!(&r, Response::Error(e) if e.code == ErrorCode::NotFound),
        "{r:?}"
    );
}

#[test]
fn export_before_the_agent_named_a_transcript_is_unavailable() {
    let d = Daemon::start();
    let mut c = d.client();
    let project = c.add_project(&d.project_dir("proj"));
    let s = match c.request(Request::CreateSession(NewSession {
        project_id: project.id,
        agent: AgentKind::ClaudeCode,
        command: Some("/bin/sh".into()),
        title: None,
        worktree: None,
        first_prompt: None,
        parent_session: None,
        requested_by_agent: false,
    })) {
        Response::Session(s) => s,
        other => panic!("create_session: {other:?}"),
    };
    let r = c.request(Request::ExportTranscript {
        session_id: s.id,
        format: ExportFormat::Markdown,
    });
    assert!(
        matches!(&r, Response::Error(e) if e.code == ErrorCode::Unavailable),
        "{r:?}"
    );
}

/// `ExportFormat` is a closed enum on the wire: a format the daemon does not know fails to decode
/// before any handler runs, and the daemon drops that connection (as for any malformed frame). The
/// other clients are not affected.
#[test]
fn export_with_an_unknown_format_is_refused_and_only_drops_that_client() {
    let d = Daemon::start();
    let mut good = d.client();
    let mut bad = d.client();
    let mut message = serde_json::to_value(Message::Request(RequestEnvelope {
        req_id: 1,
        request: Request::ExportTranscript {
            session_id: SessionId::new("x"),
            format: ExportFormat::Markdown,
        },
    }))
    .unwrap();
    let text = message.to_string();
    assert!(text.contains("\"markdown\""), "{text}");
    message = serde_json::from_str(&text.replace("\"markdown\"", "\"pdf\"")).unwrap();
    let payload = serde_json::to_vec(&message).unwrap();
    let mut raw = ((payload.len() + 1) as u32).to_be_bytes().to_vec();
    raw.push(0x01);
    raw.extend(payload);
    bad.write_raw(&raw);
    assert!(bad.wait_closed());
    assert!(matches!(good.request(Request::Ping), Response::Pong));
}

/// The answer must fit one frame (16 MiB): the oldest turns are left out, not the whole export.
#[test]
fn export_of_a_huge_transcript_keeps_the_newest_turns_inside_one_frame() {
    let d = Daemon::start();
    let mut c = d.client();
    let filler = "lorem ipsum ".repeat(16_000);
    let lines: String = (0..90)
        .map(|i| {
            format!(
                "{{\"type\":\"user\",\"message\":{{\"role\":\"user\",\"content\":\"turn {i:03} {filler}\"}}}}\n"
            )
        })
        .collect();
    assert!(lines.len() > lotse_proto::MAX_FRAME_LEN);
    let (_, s) = claude_session(&mut c, &d, &lines);
    for format in FORMATS {
        let e = export(&mut c, &s, format);
        assert!(e.content.len() < lotse_proto::MAX_FRAME_LEN, "{format:?}");
        assert!(
            e.content.contains("turn 089"),
            "{format:?}: the newest turn"
        );
        assert!(
            !e.content.contains("turn 000"),
            "{format:?}: the oldest is out"
        );
        if format != ExportFormat::JsonLines {
            assert!(
                e.content.to_lowercase().contains("older turns omitted"),
                "{format:?}"
            );
        }
    }
}

// ---- redaction table ---------------------------------------------------------------------------

fn masked(input: &str) -> (String, u32) {
    let (out, n) = redact(input);
    (out.into_owned(), n)
}

#[test]
fn redaction_table_masks_secrets() {
    let s = secrets();
    let cases: Vec<(String, String, u32)> = vec![
        // private key blocks, whole or cut off (a clipped turn loses its END line)
        (
            format!("before\n{}\nafter", s.private_key),
            "before\n[REDACTED]\nafter".into(),
            1,
        ),
        (
            "key: -----BEGIN OPENSSH PRIVATE KEY-----\nb3BlbnNzaC1rZXk\nAAAAB3Nza".into(),
            "key: [REDACTED]".into(),
            1,
        ),
        (
            "-----BEGIN PGP PRIVATE KEY BLOCK-----\nabc\n-----END PGP PRIVATE KEY BLOCK-----"
                .into(),
            "[REDACTED]".into(),
            1,
        ),
        // the rest of a key whose BEGIN line is gone (cut by a chunk, the ring buffer or a turn)
        (
            "KEYBODY00aB3xaB3xaB3xaB3x\nKEYBODY01aB3xaB3xaB3xaB3x\nZm9vYmFyYmF6\n-----END RSA PRIVATE KEY-----\nafter".into(),
            "[REDACTED]\nafter".into(),
            1,
        ),
        (
            "and then:\n-----END OPENSSH PRIVATE KEY-----".into(),
            "and then:\n[REDACTED]".into(),
            1,
        ),
        // vendor tokens
        (
            format!("use {} now", s.anthropic),
            "use [REDACTED] now".into(),
            1,
        ),
        (
            format!("sk-proj-{}", "Ab9_".repeat(12)),
            "[REDACTED]".into(),
            1,
        ),
        (s.github.clone(), "[REDACTED]".into(), 1),
        (
            format!("gho_{}", "Z9y8X7w6V5".repeat(4)),
            "[REDACTED]".into(),
            1,
        ),
        (
            format!("github_pat_{}", "11ABCDEFG0_abcdefghijklmnop"),
            "[REDACTED]".into(),
            1,
        ),
        (s.gitlab.clone(), "[REDACTED]".into(), 1),
        (format!("id {} ok", s.aws), "id [REDACTED] ok".into(), 1),
        (s.slack.clone(), "[REDACTED]".into(), 1),
        (
            format!("xoxp-{}", "987654321098-zyxwvutsrqponm"),
            "[REDACTED]".into(),
            1,
        ),
        (format!("cookie={}", s.jwt), "cookie=[REDACTED]".into(), 1),
        // headers and URLs
        (
            format!("curl -H 'Authorization: Bearer {}' x", s.bearer),
            "curl -H 'Authorization: Bearer [REDACTED]' x".into(),
            1,
        ),
        (
            format!("Authorization: Bearer {}", s.jwt),
            "Authorization: Bearer [REDACTED]".into(),
            1,
        ),
        (
            "Authorization: Basic dXNlcjpwYXNzd29yZA==".into(),
            "Authorization: Basic [REDACTED]".into(),
            1,
        ),
        (
            format!(
                "git clone https://deploy:{}@git.example.com/a.git",
                s.url_password
            ),
            "git clone https://[REDACTED]@git.example.com/a.git".into(),
            1,
        ),
        (
            format!(
                "https://{}@example.com/a/b",
                "tok".to_owned() + &"x1".repeat(12)
            ),
            "https://[REDACTED]@example.com/a/b".into(),
            1,
        ),
        // assignments
        (
            format!("ANTHROPIC_API_KEY={}", s.anthropic),
            "ANTHROPIC_API_KEY=[REDACTED]".into(),
            1,
        ),
        (
            "export STRIPE_SECRET=abc123".into(),
            "export STRIPE_SECRET=[REDACTED]".into(),
            1,
        ),
        (
            "GITHUB_TOKEN=abc".into(),
            "GITHUB_TOKEN=[REDACTED]".into(),
            1,
        ),
        (
            "DB_PASSWORD='a b c'".into(),
            "DB_PASSWORD=[REDACTED]".into(),
            1,
        ),
        (
            "password = \"hunter2\"".into(),
            "password = [REDACTED]".into(),
            1,
        ),
        (
            "db_password=hunter2".into(),
            "db_password=[REDACTED]".into(),
            1,
        ),
        ("api_key=abcd1234".into(), "api_key=[REDACTED]".into(), 1),
        (
            "A_KEY=1\nB_TOKEN=2\nC_SECRET=3".into(),
            "A_KEY=[REDACTED]\nB_TOKEN=[REDACTED]\nC_SECRET=[REDACTED]".into(),
            3,
        ),
        // command line flags
        (
            "mytool --password hunter2 --api-key=abc12345".into(),
            "mytool --password [REDACTED] --api-key=[REDACTED]".into(),
            2,
        ),
        // JSON
        (
            r#"{"password": "hunter2", "api_key":"abcd", "name": "x"}"#.into(),
            r#"{"password": "[REDACTED]", "api_key":"[REDACTED]", "name": "x"}"#.into(),
            2,
        ),
        // a value that is also a vendor token is counted once
        (
            format!("OPENAI_API_KEY=sk-{}", "a1".repeat(15)),
            "OPENAI_API_KEY=[REDACTED]".into(),
            1,
        ),
    ];
    for (input, want, count) in cases {
        let (got, n) = masked(&input);
        assert_eq!((got.as_str(), n), (want.as_str(), count), "input: {input}");
        // masking again changes nothing
        assert_eq!(masked(&got), (got.clone(), 0), "not idempotent: {got}");
    }
}

#[test]
fn redaction_table_leaves_near_misses_alone() {
    let s = secrets();
    let near_misses: Vec<String> = vec![
        // looks like a prefix, is not a token
        "the sk-learn pipeline".into(),
        "sk-assessment-framework-documentation".into(),
        "risk-assessment-framework-documentation-v2".into(),
        "ask-user-question-tool-1234567890".into(),
        "ghp_short".into(),
        format!("ghp_{}", "a".repeat(19)),
        "github_pat_short".into(),
        "glpat-short".into(),
        "xoxb-short".into(),
        "AKIAIOSFODNN7EXAMPL".into(),
        "akiaiosfodnn7example".into(),
        format!("X{}", s.aws),
        "eyJhbGciOiJIUzI1NiJ9".into(),
        "eyJhbGciOi.abc.def".into(),
        "Bearer authentication is required".into(),
        "Bearer of bad news".into(),
        // URLs without credentials
        "git@github.com:org/repo.git".into(),
        "https://example.com/@user/pkg".into(),
        "ssh://git@github.com/org/repo".into(),
        "postgres://user@localhost/db".into(),
        "https://example.com:8080/path?x=1".into(),
        // names that only look like secret names, and values that are references
        "KEYBOARD=us".into(),
        "MONKEY=banana".into(),
        "TOKEN_COUNT=5".into(),
        "max_tokens=4096".into(),
        "sort_key=name".into(),
        "PASSWORD_FILE=/run/secrets/db".into(),
        "API_KEY=$OTHER_VAR".into(),
        "API_KEY=${VAULT_KEY}".into(),
        "API_KEY=<your key here>".into(),
        "API_KEY=".into(),
        "author=bob".into(),
        "--token-count 5 --password-file x".into(),
        r#"{"token_count": 5, "name": "x"}"#.into(),
        // other PEM blocks
        "-----BEGIN PUBLIC KEY-----\nabc\n-----END PUBLIC KEY-----".into(),
        "-----BEGIN CERTIFICATE-----\nabc\n-----END CERTIFICATE-----".into(),
        // prose
        "set your password in the settings and keep the token budget low".into(),
        "".into(),
    ];
    for input in near_misses {
        assert_eq!(masked(&input), (input.clone(), 0), "changed: {input}");
    }
}
