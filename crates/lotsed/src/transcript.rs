//! Agent transcripts (Show Transcript): which file the daemon may read, and its last turns.
//!
//! The path comes from an agent's hook payload (`transcript_path`), i.e. from outside the daemon,
//! so it is never trusted: [`validate_path`] only lets a regular `*.jsonl` file below the user's
//! home through, after resolving symlinks. The file is read from its tail, bounded in bytes, turns
//! and text length, and only the conversation is kept (no tool output, no thinking).
//!
//! - Claude Code: one JSON object per line, `type` `user` / `assistant`, `message.content` text and
//!   `tool_use` blocks. Checked against the transcripts Claude Code writes today.
//! - Codex: its rollout file, `response_item` lines with `payload.type` `message` / `function_call`.
//!   **Unverified:** shaped from Codex's rollout format as remembered; no live Codex run was
//!   available (tests never run an agent). An unreadable line is skipped, never an error.
//! - OpenCode: its plugin payload carries no transcript path, so there is nothing to read.

use lotse_proto::{
    AgentKind, Transcript, TranscriptRole, TranscriptSource, TranscriptTool, TranscriptTurn, UnixMs,
};
use serde_json::Value;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

pub const DEFAULT_TURNS: usize = 50;
pub const MAX_TURNS: usize = 200;
/// Bytes read from the end of the file; older turns are left out.
const TAIL_BYTES: u64 = 2 * 1024 * 1024;
/// Characters kept of one turn's text.
const MAX_TEXT: usize = 4_000;
/// Characters kept of a tool call's summary.
const MAX_SUMMARY: usize = 160;
/// Text bytes in one answer (the newest turns win).
const MAX_TOTAL_TEXT: usize = 256 * 1024;

/// `raw` as a path the daemon may read: absolute, a regular file, named `*.jsonl`, and inside
/// `home` once symlinks are resolved (a link in the home folder that points elsewhere, or at a
/// file that is not `*.jsonl`, is refused). Returns the resolved path.
pub fn validate_path(raw: &str, home: &Path) -> Option<PathBuf> {
    let path = Path::new(raw);
    if !path.is_absolute() || !is_jsonl(path) {
        return None;
    }
    let real = std::fs::canonicalize(path).ok()?;
    let home = std::fs::canonicalize(home).ok()?;
    if !real.starts_with(&home) || real == home || !is_jsonl(&real) {
        return None;
    }
    std::fs::metadata(&real).ok()?.is_file().then_some(real)
}

fn is_jsonl(p: &Path) -> bool {
    p.extension().is_some_and(|e| e == "jsonl")
}

/// The last `limit` turns (at most [`MAX_TURNS`]) of the transcript at `path`. The path is checked
/// again (it may have been swapped for a link since it was recorded).
pub fn read_turns(
    path: &Path,
    agent: AgentKind,
    limit: usize,
    home: &Path,
) -> std::io::Result<Transcript> {
    let denied = || std::io::Error::new(std::io::ErrorKind::PermissionDenied, "transcript path");
    let real = validate_path(&path.to_string_lossy(), home).ok_or_else(denied)?;
    let mut file = std::fs::File::open(&real)?;
    let len = file.metadata()?.len();
    let start = len.saturating_sub(TAIL_BYTES);
    file.seek(SeekFrom::Start(start))?;
    let mut bytes = Vec::with_capacity((len - start) as usize);
    file.take(TAIL_BYTES).read_to_end(&mut bytes)?;
    let text = String::from_utf8_lossy(&bytes);
    // a tail starts mid-line: drop the partial first line
    let text = if start > 0 {
        text.split_once('\n').map_or("", |(_, rest)| rest)
    } else {
        &text
    };
    let mut turns: Vec<TranscriptTurn> = text
        .lines()
        .flat_map(|line| parse_line(agent, line))
        .collect();
    let limit = limit.clamp(1, MAX_TURNS);
    let mut truncated = start > 0 || turns.len() > limit;
    if turns.len() > limit {
        turns.drain(..turns.len() - limit);
    }
    // the newest turns win when the answer would get too big
    let mut size = 0;
    let keep_from = turns
        .iter()
        .rposition(|t| {
            size += t.text.len() + t.tool.as_ref().map_or(0, |t| t.summary.len());
            size > MAX_TOTAL_TEXT
        })
        .map_or(0, |i| i + 1);
    if keep_from > 0 {
        turns.drain(..keep_from);
        truncated = true;
    }
    Ok(Transcript {
        available: true,
        turns,
        truncated,
        source: source_of(agent),
    })
}

/// Where `agent`'s transcript comes from (Beta adds OpenCode and scrollback in `transcript_sources`).
pub fn source_of(agent: AgentKind) -> TranscriptSource {
    match agent {
        AgentKind::Codex => TranscriptSource::Codex,
        AgentKind::OpenCode => TranscriptSource::OpenCode,
        AgentKind::ClaudeCode => TranscriptSource::Claude,
        AgentKind::Terminal => TranscriptSource::Scrollback,
    }
}

fn parse_line(agent: AgentKind, line: &str) -> Vec<TranscriptTurn> {
    if line.trim().is_empty() {
        return Vec::new();
    }
    let Ok(v) = serde_json::from_str::<Value>(line) else {
        return Vec::new();
    };
    match agent {
        AgentKind::ClaudeCode => claude_turns(&v),
        AgentKind::Codex => codex_turns(&v),
        AgentKind::Terminal | AgentKind::OpenCode => Vec::new(),
    }
}

fn turn(role: TranscriptRole, text: &str, at: Option<UnixMs>) -> Option<TranscriptTurn> {
    let text = text.trim();
    (!text.is_empty()).then(|| TranscriptTurn {
        role,
        text: clip(text, MAX_TEXT),
        tool: None,
        at,
        approval: None,
        sub_agent: None,
    })
}

fn tool_turn(name: &str, summary: &str, at: Option<UnixMs>) -> TranscriptTurn {
    TranscriptTurn {
        role: TranscriptRole::Tool,
        text: String::new(),
        tool: Some(TranscriptTool {
            name: clip(name, 60),
            summary: clip(summary.trim(), MAX_SUMMARY),
            status: None,
            result: None,
        }),
        at,
        approval: None,
        sub_agent: None,
    }
}

/// At most `max` characters; an ellipsis marks a cut.
fn clip(s: &str, max: usize) -> String {
    match s.char_indices().nth(max) {
        Some((i, _)) => format!("{}…", &s[..i]),
        None => s.to_owned(),
    }
}

/// The `tool_input` keys that say what a call is about, most telling first.
const SUMMARY_KEYS: [&str; 8] = [
    "command",
    "file_path",
    "notebook_path",
    "path",
    "url",
    "pattern",
    "query",
    "description",
];

fn summarize_input(input: &Value) -> String {
    SUMMARY_KEYS
        .iter()
        .find_map(|k| input.get(k).and_then(command_text))
        .unwrap_or_default()
}

/// A string, or a command line given as an argv array (`["bash", "-lc", "ls"]` shows as `ls`).
fn command_text(v: &Value) -> Option<String> {
    match v {
        Value::String(s) => Some(s.clone()),
        Value::Array(items) => {
            let words: Vec<&str> = items.iter().filter_map(Value::as_str).collect();
            let shell = matches!(words.as_slice(), [sh, "-lc" | "-c", ..] if sh.ends_with("sh"));
            Some(if shell {
                words[2..].join(" ")
            } else {
                words.join(" ")
            })
        }
        _ => None,
    }
}

fn claude_turns(v: &Value) -> Vec<TranscriptTurn> {
    let flag = |k: &str| v.get(k).and_then(Value::as_bool).unwrap_or(false);
    if flag("isSidechain") || flag("isMeta") {
        return Vec::new();
    }
    let at = v
        .get("timestamp")
        .and_then(Value::as_str)
        .and_then(parse_iso_utc_ms);
    let content = v.get("message").and_then(|m| m.get("content"));
    let role = match v.get("type").and_then(Value::as_str) {
        Some("user") => TranscriptRole::User,
        Some("assistant") => TranscriptRole::Assistant,
        _ => return Vec::new(),
    };
    match content {
        Some(Value::String(s)) => turn(role, s, at).into_iter().collect(),
        Some(Value::Array(blocks)) => blocks
            .iter()
            .filter_map(|b| match b.get("type").and_then(Value::as_str) {
                Some("text") => turn(role, b.get("text")?.as_str()?, at),
                Some("tool_use") if role == TranscriptRole::Assistant => Some(tool_turn(
                    b.get("name").and_then(Value::as_str).unwrap_or("tool"),
                    &b.get("input").map(summarize_input).unwrap_or_default(),
                    at,
                )),
                _ => None, // tool results, thinking, images
            })
            .collect(),
        _ => Vec::new(),
    }
}

/// What Codex injects as the first "user" messages: not part of the conversation.
fn codex_boilerplate(text: &str) -> bool {
    let t = text.trim_start();
    t.starts_with("<environment_context>")
        || t.starts_with("<user_instructions>")
        || t.starts_with("# AGENTS.md")
}

fn codex_turns(v: &Value) -> Vec<TranscriptTurn> {
    if v.get("type").and_then(Value::as_str) != Some("response_item") {
        return Vec::new();
    }
    let at = v
        .get("timestamp")
        .and_then(Value::as_str)
        .and_then(parse_iso_utc_ms);
    let Some(p) = v.get("payload") else {
        return Vec::new();
    };
    match p.get("type").and_then(Value::as_str) {
        Some("message") => {
            let role = match p.get("role").and_then(Value::as_str) {
                Some("user") => TranscriptRole::User,
                Some("assistant") => TranscriptRole::Assistant,
                _ => return Vec::new(),
            };
            let text: Vec<&str> = p
                .get("content")
                .and_then(Value::as_array)
                .map(|items| {
                    items
                        .iter()
                        .filter_map(|c| c.get("text").and_then(Value::as_str))
                        .collect()
                })
                .unwrap_or_default();
            let text = text.join("\n");
            if codex_boilerplate(&text) {
                return Vec::new();
            }
            turn(role, &text, at).into_iter().collect()
        }
        Some("function_call") => {
            let name = p.get("name").and_then(Value::as_str).unwrap_or("tool");
            // `arguments` is a JSON document inside a string
            let args = p
                .get("arguments")
                .and_then(Value::as_str)
                .and_then(|a| serde_json::from_str::<Value>(a).ok())
                .unwrap_or(Value::Null);
            vec![tool_turn(name, &summarize_input(&args), at)]
        }
        Some("local_shell_call") => {
            let cmd = p
                .get("action")
                .and_then(|a| a.get("command"))
                .and_then(command_text)
                .unwrap_or_default();
            vec![tool_turn("shell", &cmd, at)]
        }
        _ => Vec::new(),
    }
}

/// `2026-11-02T10:11:12.345Z` (UTC, as the agents write it) → Unix milliseconds.
pub fn parse_iso_utc_ms(s: &str) -> Option<u64> {
    let b = s.as_bytes();
    let num = |from: usize, len: usize| -> Option<i64> {
        let part = s.get(from..from + len)?;
        part.bytes()
            .all(|c| c.is_ascii_digit())
            .then(|| part.parse().ok())?
    };
    if b.len() < 19 || b[4] != b'-' || b[7] != b'-' || !matches!(b[10], b'T' | b' ') {
        return None;
    }
    let (y, m, d) = (num(0, 4)?, num(5, 2)?, num(8, 2)?);
    let (h, min, sec) = (num(11, 2)?, num(14, 2)?, num(17, 2)?);
    if !(1..=12).contains(&m) || !(1..=31).contains(&d) || h > 23 || min > 59 || sec > 60 {
        return None;
    }
    let mut ms = 0i64;
    if b.get(19) == Some(&b'.') {
        let digits: String = s[20..]
            .chars()
            .take_while(char::is_ascii_digit)
            .take(3)
            .collect();
        ms = format!("{digits:0<3}").parse().ok()?;
    }
    // days from civil (Howard Hinnant)
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146_097 + doe - 719_468;
    u64::try_from(((days * 24 + h) * 60 + min) * 60 + sec)
        .ok()?
        .checked_mul(1000)?
        .checked_add(ms as u64)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::symlink;

    fn home() -> tempfile::TempDir {
        tempfile::tempdir().unwrap()
    }

    fn write(dir: &Path, name: &str, text: &str) -> PathBuf {
        let p = dir.join(name);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(&p, text).unwrap();
        p
    }

    fn ok(raw: &Path, home: &Path) -> bool {
        validate_path(&raw.to_string_lossy(), home).is_some()
    }

    #[test]
    fn a_jsonl_file_below_home_is_accepted_and_resolved() {
        let h = home();
        let p = write(h.path(), ".claude/projects/x/s.jsonl", "{}\n");
        let real = validate_path(&p.to_string_lossy(), h.path()).unwrap();
        assert_eq!(real, std::fs::canonicalize(&p).unwrap());
    }

    #[test]
    fn paths_outside_home_are_rejected() {
        let (h, other) = (home(), home());
        let outside = write(other.path(), "s.jsonl", "{}\n");
        assert!(!ok(&outside, h.path()));
        // `..` out of home resolves before the check
        let sneaky = h.path().join("..").join(other.path().file_name().unwrap());
        assert!(!ok(&sneaky.join("s.jsonl"), h.path()));
        // a sibling folder whose name starts with the home folder's name is not inside it
        let sibling = PathBuf::from(format!("{}-evil", h.path().display()));
        std::fs::create_dir_all(&sibling).unwrap();
        let evil = write(&sibling, "s.jsonl", "{}\n");
        assert!(!ok(&evil, h.path()));
        std::fs::remove_dir_all(&sibling).unwrap();
    }

    #[test]
    fn only_jsonl_regular_absolute_files_are_accepted() {
        let h = home();
        assert!(!ok(&write(h.path(), "notes.txt", "x"), h.path()));
        assert!(!ok(&write(h.path(), "s.jsonl.bak", "x"), h.path()));
        assert!(!ok(&write(h.path(), "noext", "x"), h.path()));
        std::fs::create_dir(h.path().join("dir.jsonl")).unwrap();
        assert!(!ok(&h.path().join("dir.jsonl"), h.path()), "a directory");
        assert!(!ok(&h.path().join("missing.jsonl"), h.path()));
        assert!(validate_path("relative/s.jsonl", h.path()).is_none());
        assert!(validate_path("", h.path()).is_none());
    }

    #[test]
    fn a_symlink_cannot_lead_out_of_home_or_to_a_non_jsonl_file() {
        let (h, other) = (home(), home());
        let secret = write(other.path(), "secret.jsonl", "{}\n");
        let link = h.path().join("link.jsonl");
        symlink(&secret, &link).unwrap();
        assert!(!ok(&link, h.path()), "link to a file outside home");

        let dir_link = h.path().join("elsewhere");
        symlink(other.path(), &dir_link).unwrap();
        assert!(
            !ok(&dir_link.join("secret.jsonl"), h.path()),
            "link to a folder outside home"
        );

        let passwd = write(h.path(), "passwd.txt", "root\n");
        let disguised = h.path().join("disguised.jsonl");
        symlink(&passwd, &disguised).unwrap();
        assert!(
            !ok(&disguised, h.path()),
            "link to a file that is not *.jsonl"
        );

        // a link that stays inside home is fine
        let real = write(h.path(), "real/s.jsonl", "{}\n");
        let inside = h.path().join("inside.jsonl");
        symlink(&real, &inside).unwrap();
        assert!(ok(&inside, h.path()));
    }

    #[test]
    fn read_turns_checks_the_path_again() {
        let (h, other) = (home(), home());
        let outside = write(other.path(), "s.jsonl", "{}\n");
        let err = read_turns(&outside, AgentKind::ClaudeCode, 10, h.path()).unwrap_err();
        assert_eq!(err.kind(), std::io::ErrorKind::PermissionDenied);
    }

    const CLAUDE: &str = r#"{"type":"summary","summary":"x"}
{"type":"user","timestamp":"2026-11-02T10:00:00.000Z","message":{"role":"user","content":"Fix the invoice rounding"}}
{"type":"user","isMeta":true,"message":{"role":"user","content":"<system-reminder>hidden</system-reminder>"}}
{"type":"assistant","timestamp":"2026-11-02T10:00:05.250Z","message":{"role":"assistant","model":"claude-opus-4-5","content":[{"type":"thinking","thinking":"secret thoughts"},{"type":"text","text":"I'll look at the rounding code."},{"type":"tool_use","id":"t1","name":"Bash","input":{"command":"cargo test invoice","description":"run tests"}}]}}
{"type":"user","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"t1","content":"HUGE OUTPUT"}]}}
{"type":"assistant","isSidechain":true,"message":{"role":"assistant","content":[{"type":"text","text":"sub-agent chatter"}]}}
not json at all
{"type":"assistant","message":{"role":"assistant","content":[{"type":"tool_use","name":"Edit","input":{"file_path":"/a/b.rs","old_string":"x","new_string":"y"}},{"type":"text","text":"Done."}]}}
"#;

    #[test]
    fn claude_turns_keep_the_conversation_and_drop_the_noise() {
        let h = home();
        let p = write(h.path(), "s.jsonl", CLAUDE);
        let t = read_turns(&p, AgentKind::ClaudeCode, 50, h.path()).unwrap();
        assert!(t.available && !t.truncated);
        let got: Vec<_> = t
            .turns
            .iter()
            .map(|t| {
                (
                    t.role,
                    t.text.as_str(),
                    t.tool
                        .as_ref()
                        .map(|x| (x.name.as_str(), x.summary.as_str())),
                )
            })
            .collect();
        use TranscriptRole::*;
        assert_eq!(
            got,
            [
                (User, "Fix the invoice rounding", None),
                (Assistant, "I'll look at the rounding code.", None),
                (Tool, "", Some(("Bash", "cargo test invoice"))),
                (Tool, "", Some(("Edit", "/a/b.rs"))),
                (Assistant, "Done.", None),
            ]
        );
        assert_eq!(t.turns[0].at, Some(1_793_613_600_000));
        assert_eq!(t.turns[1].at, Some(1_793_613_605_250));
        let all = serde_json::to_string(&t).unwrap();
        assert!(!all.contains("HUGE OUTPUT") && !all.contains("secret thoughts"));
    }

    #[test]
    fn the_last_n_turns_win_and_long_text_is_clipped() {
        let h = home();
        let mut text = String::new();
        for i in 0..30 {
            text += &format!(
                "{{\"type\":\"user\",\"message\":{{\"role\":\"user\",\"content\":\"turn {i}\"}}}}\n"
            );
        }
        text += &format!(
            "{{\"type\":\"assistant\",\"message\":{{\"role\":\"assistant\",\"content\":\"{}\"}}}}\n",
            "x".repeat(10_000)
        );
        let p = write(h.path(), "s.jsonl", &text);
        let t = read_turns(&p, AgentKind::ClaudeCode, 5, h.path()).unwrap();
        assert_eq!(t.turns.len(), 5);
        assert!(t.truncated);
        assert_eq!(t.turns[0].text, "turn 26");
        let last = t.turns.last().unwrap();
        assert_eq!(
            last.text.chars().count(),
            MAX_TEXT + 1,
            "clipped with an ellipsis"
        );
        assert!(last.text.ends_with('…'));
        // an absurd limit is held to the maximum
        let many = read_turns(&p, AgentKind::ClaudeCode, 10_000, h.path()).unwrap();
        assert_eq!(many.turns.len(), 31);
    }

    #[test]
    fn a_big_file_is_read_from_its_tail_and_the_cut_line_is_dropped() {
        let h = home();
        let filler = format!(
            "{{\"type\":\"user\",\"message\":{{\"role\":\"user\",\"content\":\"{}\"}}}}\n",
            "f".repeat(1000)
        );
        let mut text = filler.repeat(3_000); // ~3 MB > TAIL_BYTES
        text += "{\"type\":\"assistant\",\"message\":{\"role\":\"assistant\",\"content\":\"the end\"}}\n";
        let p = write(h.path(), "big.jsonl", &text);
        let t = read_turns(&p, AgentKind::ClaudeCode, 3, h.path()).unwrap();
        assert!(t.truncated);
        assert_eq!(t.turns.last().unwrap().text, "the end");
        assert_eq!(t.turns.len(), 3);
        // the answer stays bounded
        let big = read_turns(&p, AgentKind::ClaudeCode, MAX_TURNS, h.path()).unwrap();
        let bytes: usize = big.turns.iter().map(|t| t.text.len()).sum();
        assert!(bytes <= MAX_TOTAL_TEXT, "{bytes}");
    }

    #[test]
    fn codex_rollout_lines_become_turns() {
        let h = home();
        let rollout = r#"{"timestamp":"2026-11-02T10:00:00.000Z","type":"session_meta","payload":{"id":"x"}}
{"timestamp":"2026-11-02T10:00:01.000Z","type":"response_item","payload":{"type":"message","role":"user","content":[{"type":"input_text","text":"<environment_context>cwd</environment_context>"}]}}
{"timestamp":"2026-11-02T10:00:02.000Z","type":"response_item","payload":{"type":"message","role":"user","content":[{"type":"input_text","text":"add a test"}]}}
{"timestamp":"2026-11-02T10:00:03.000Z","type":"response_item","payload":{"type":"function_call","name":"shell","arguments":"{\"command\":[\"bash\",\"-lc\",\"cargo test\"]}","call_id":"c1"}}
{"timestamp":"2026-11-02T10:00:04.000Z","type":"response_item","payload":{"type":"function_call_output","call_id":"c1","output":"BIG"}}
{"timestamp":"2026-11-02T10:00:05.000Z","type":"response_item","payload":{"type":"message","role":"assistant","content":[{"type":"output_text","text":"Added."}]}}
{"type":"event_msg","payload":{"type":"token_count"}}
"#;
        let p = write(h.path(), "rollout.jsonl", rollout);
        let t = read_turns(&p, AgentKind::Codex, 50, h.path()).unwrap();
        let got: Vec<_> = t.turns.iter().map(|t| (t.role, t.text.as_str())).collect();
        use TranscriptRole::*;
        assert_eq!(
            got,
            [(User, "add a test"), (Tool, ""), (Assistant, "Added.")]
        );
        assert_eq!(t.turns[1].tool.as_ref().unwrap().summary, "cargo test");
    }

    #[test]
    fn opencode_and_terminal_have_nothing_to_parse() {
        let h = home();
        let p = write(h.path(), "s.jsonl", CLAUDE);
        for agent in [AgentKind::OpenCode, AgentKind::Terminal] {
            assert!(
                read_turns(&p, agent, 50, h.path())
                    .unwrap()
                    .turns
                    .is_empty()
            );
        }
    }

    #[test]
    fn iso_timestamps_parse_to_utc_milliseconds() {
        assert_eq!(parse_iso_utc_ms("1970-01-01T00:00:00Z"), Some(0));
        assert_eq!(
            parse_iso_utc_ms("2026-11-02T10:00:00.5Z"),
            Some(1_793_613_600_500)
        );
        assert_eq!(
            parse_iso_utc_ms("2024-02-29T23:59:59.999Z"),
            Some(1_709_251_199_999)
        );
        assert_eq!(parse_iso_utc_ms("2026-13-02T10:00:00Z"), None);
        assert_eq!(parse_iso_utc_ms("yesterday"), None);
        assert_eq!(
            parse_iso_utc_ms("1969-12-31T23:59:59Z"),
            None,
            "before the epoch"
        );
    }
}
