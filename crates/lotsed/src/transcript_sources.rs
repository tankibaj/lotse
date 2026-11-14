//! Beta (wp07): where a session's transcript comes from, behind `get_transcript` and
//! `export_transcript`.
//!
//! - **Claude Code, and Codex once a hook named its file:** the file the hook announced.
//! - **Codex without a hook yet:** the newest `rollout-*.jsonl` under the pane's
//!   `CODEX_HOME/sessions` (the mirror Lotse gave the pane links `sessions` to the real home) that
//!   was written for this session's folder. A rollout of another folder is never shown. Two Codex
//!   panes in one folder cannot be told apart: the newest wins.
//! - **OpenCode and plain terminals:** the scrollback in the session's ring buffer, ANSI stripped, in
//!   chunks of lines. No timestamps, no tool calls: only a text view.
//!
//! The permission decisions of `perm_events` (answered through Lotse) are merged in by time as
//! `approval` turns. A prompt answered in the terminal has no recorded decision and is left out.
//! Every text is masked ([`crate::redact`]) before it leaves.

use crate::db::{Db, PermEventRow};
use crate::redact::redact;
use crate::session::SessionHandle;
use crate::state::State;
use crate::transcript::{self, AgentDirs, DEFAULT_TURNS, EXPORT};
use lotse_proto::{
    AgentKind, ErrorCode, ProtoError, Response, SessionId, Transcript, TranscriptApproval,
    TranscriptRole, TranscriptSource, TranscriptTurn,
};
use std::io::{BufRead, BufReader, Error, ErrorKind, Read};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::SystemTime;

/// Bytes read from the end of a transcript file for an export.
pub const EXPORT_READ_BYTES: u64 = 20 * 1024 * 1024;
/// Rollouts looked at (newest days first) when a Codex pane has no announced file.
const ROLLOUT_CANDIDATES: usize = 20;
/// Entries read from one folder of the rollout tree.
const MAX_DIR_ENTRIES: usize = 2_000;
/// Longest first line of a rollout that is read to find its folder.
const MAX_META_LINE: u64 = 256 * 1024;
/// A scrollback turn holds at most this many lines …
const CHUNK_LINES: usize = 40;
/// … and about this many characters.
const CHUNK_CHARS: usize = 3_500;

/// How much of the transcript is wanted.
#[derive(Clone, Copy)]
pub enum Scope {
    /// The newest turns (at most `limit`, clamped), as [`transcript::VIEW`] clips them.
    Recent(usize),
    /// Everything up to [`EXPORT_READ_BYTES`], as [`EXPORT`] clips it.
    Full,
}

pub struct Collected {
    pub transcript: Transcript,
    /// The model the agent named last (not part of the wire `Transcript`; exports show it).
    pub model: Option<String>,
    /// Secrets masked ([`Scope::Full`] only).
    pub redactions: u32,
}

/// `get_transcript`.
pub async fn get(state: &Arc<State>, id: &SessionId, limit: Option<u32>) -> Response {
    let Some(h) = state.session(id) else {
        return error(ErrorCode::NotFound, "session not found");
    };
    let limit = limit.map_or(DEFAULT_TURNS, |l| l as usize);
    match collect(state, &h, Scope::Recent(limit)).await {
        Ok(c) => Response::Transcript(c.transcript),
        Err(e) => error(ErrorCode::Unavailable, format!("transcript: {e}")),
    }
}

pub fn error(code: ErrorCode, message: impl Into<String>) -> Response {
    Response::Error(ProtoError {
        code,
        message: message.into(),
    })
}

/// The transcript of `h`, from the source its agent has, with the approvals merged in.
pub async fn collect(
    state: &Arc<State>,
    h: &Arc<SessionHandle>,
    scope: Scope,
) -> std::io::Result<Collected> {
    let meta = h.snapshot();
    let announced = h.transcript();
    let inputs = Inputs {
        session: meta.id,
        agent: meta.agent,
        scrollback: if announced.is_none()
            && matches!(meta.agent, AgentKind::Terminal | AgentKind::OpenCode)
        {
            h.scrollback()
        } else {
            Vec::new()
        },
        announced,
        cwd: h.cwd(),
        dirs: h.agent_dirs(),
        home: state.config().home.clone(),
    };
    let db = state.db().clone();
    tokio::task::spawn_blocking(move || read(&inputs, &db, scope))
        .await
        .map_err(Error::other)?
}

/// What is read off the session before the blocking work starts.
struct Inputs {
    session: SessionId,
    agent: AgentKind,
    announced: Option<(AgentKind, PathBuf)>,
    cwd: PathBuf,
    /// The session's login environment: where its agents keep their files.
    dirs: AgentDirs,
    home: Option<PathBuf>,
    /// Only filled for the agents whose transcript is their scrollback.
    scrollback: Vec<u8>,
}

enum Pick {
    File(AgentKind, PathBuf),
    Scrollback,
    /// The agent has a transcript, but it has not said where yet.
    Nothing(TranscriptSource),
}

fn pick(inp: &Inputs) -> Pick {
    if let Some((agent, path)) = &inp.announced {
        return Pick::File(*agent, path.clone());
    }
    match inp.agent {
        AgentKind::ClaudeCode => Pick::Nothing(TranscriptSource::Claude),
        AgentKind::Codex => {
            let codex_home = inp
                .dirs
                .codex_home
                .as_ref()
                .map(PathBuf::from)
                .or_else(|| inp.dirs.home.as_ref().map(|h| h.join(".codex")))
                .or_else(|| inp.home.as_ref().map(|h| h.join(".codex")));
            match codex_home.and_then(|h| newest_rollout(&h.join("sessions"), &inp.cwd)) {
                Some(path) => Pick::File(AgentKind::Codex, path),
                None => Pick::Nothing(TranscriptSource::Codex),
            }
        }
        AgentKind::Terminal | AgentKind::OpenCode => Pick::Scrollback,
    }
}

fn read(inp: &Inputs, db: &Db, scope: Scope) -> std::io::Result<Collected> {
    // secrets already masked before the text was cut into turns
    let mut premasked = 0;
    let (mut turns, model, cut, source) = match pick(inp) {
        Pick::File(agent, path) => {
            let home = inp
                .home
                .as_deref()
                .ok_or_else(|| Error::new(ErrorKind::PermissionDenied, "no home folder"))?;
            let max = match scope {
                Scope::Recent(_) => transcript::TAIL_BYTES,
                Scope::Full => EXPORT_READ_BYTES,
            };
            let parsed = transcript::parse_file(&path, agent, home, &inp.dirs, max)?;
            (
                parsed.turns,
                parsed.model,
                parsed.cut,
                transcript::source_of(agent),
            )
        }
        Pick::Scrollback => {
            let (turns, masked) = scrollback_turns(&inp.scrollback);
            premasked = masked;
            (turns, None, false, TranscriptSource::Scrollback)
        }
        Pick::Nothing(source) => {
            return Ok(Collected {
                transcript: Transcript {
                    available: false,
                    turns: Vec::new(),
                    truncated: false,
                    source,
                },
                model: None,
                redactions: 0,
            });
        }
    };
    // a scrollback has no times to merge by
    if source != TranscriptSource::Scrollback {
        let events = db.list_perm_events(&inp.session).map_err(Error::other)?;
        merge_approvals(&mut turns, &events);
    }
    Ok(match scope {
        Scope::Recent(limit) => Collected {
            transcript: transcript::window(turns, cut, limit, source),
            model,
            redactions: 0,
        },
        Scope::Full => {
            let redactions = premasked + transcript::shape(&mut turns, EXPORT);
            Collected {
                transcript: Transcript {
                    available: true,
                    turns,
                    truncated: cut,
                    source,
                },
                model,
                redactions,
            }
        }
    })
}

/// Put the decisions made through Lotse into `turns` as `approval` turns, each before the first turn
/// that happened after it. A prompt without a recorded decision is left out.
pub fn merge_approvals(turns: &mut Vec<TranscriptTurn>, events: &[PermEventRow]) {
    let mut decided: Vec<TranscriptTurn> = events
        .iter()
        .filter_map(|e| {
            let decision = e.decision?;
            Some(TranscriptTurn {
                role: TranscriptRole::Approval,
                text: String::new(),
                tool: None,
                at: Some(e.decided_at.unwrap_or(e.asked_at)),
                approval: Some(TranscriptApproval {
                    decision,
                    via: e.via,
                    command: if e.summary.is_empty() {
                        e.tool.clone()
                    } else {
                        format!("{}({})", e.tool, e.summary)
                    },
                }),
                sub_agent: None,
            })
        })
        .collect();
    decided.sort_by_key(|t| t.at);
    for approval in decided {
        let at = approval.at.unwrap_or_default();
        let place = turns
            .iter()
            .position(|t| t.at.is_some_and(|t| t > at))
            .unwrap_or(turns.len());
        turns.insert(place, approval);
    }
}

// ---- Codex ---------------------------------------------------------------------------------------

/// The newest rollout below `sessions` (`YYYY/MM/DD/rollout-*.jsonl`) that was written for the
/// folder `cwd`; one that does not say which folder is the fallback. Looks at the newest days only.
fn newest_rollout(sessions: &Path, cwd: &Path) -> Option<PathBuf> {
    let mut found: Vec<(SystemTime, PathBuf)> = Vec::new();
    'days: for year in subdirs_newest_first(sessions) {
        for month in subdirs_newest_first(&year) {
            for day in subdirs_newest_first(&month) {
                let Ok(entries) = std::fs::read_dir(&day) else {
                    continue;
                };
                for entry in entries.filter_map(Result::ok).take(MAX_DIR_ENTRIES) {
                    let name = entry.file_name();
                    let name = name.to_string_lossy();
                    if !(name.starts_with("rollout-") && name.ends_with(".jsonl")) {
                        continue;
                    }
                    let path = entry.path();
                    // a rollout that is a link is never read (and `validate_path` refuses it)
                    if let Ok(meta) = std::fs::symlink_metadata(&path)
                        && meta.is_file()
                        && let Ok(modified) = meta.modified()
                    {
                        found.push((modified, path));
                    }
                }
                if found.len() >= ROLLOUT_CANDIDATES {
                    break 'days;
                }
            }
        }
    }
    found.sort_by_key(|(modified, _)| std::cmp::Reverse(*modified));
    let wanted = std::fs::canonicalize(cwd).unwrap_or_else(|_| cwd.to_owned());
    let mut unknown = None;
    for (_, path) in found.into_iter().take(ROLLOUT_CANDIDATES) {
        match rollout_cwd(&path) {
            Some(dir) if same_dir(&dir, &wanted) => return Some(path),
            Some(_) => {}
            None => {
                unknown.get_or_insert(path);
            }
        }
    }
    unknown
}

/// Subfolders of `dir`, the newest (highest name) first: the year, month and day folders sort by name.
fn subdirs_newest_first(dir: &Path) -> Vec<PathBuf> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut dirs: Vec<PathBuf> = entries
        .filter_map(Result::ok)
        .take(MAX_DIR_ENTRIES)
        .map(|e| e.path())
        .filter(|p| p.is_dir())
        .collect();
    dirs.sort_by(|a, b| b.cmp(a));
    dirs
}

/// The folder a rollout was started in: `payload.cwd` of its first line (`session_meta`).
fn rollout_cwd(path: &Path) -> Option<PathBuf> {
    // the mirror links the `sessions` folder: resolve the folders, but never a link as the file
    let real = std::fs::canonicalize(path.parent()?)
        .ok()?
        .join(path.file_name()?);
    let file = transcript::open_nofollow(&real).ok()?;
    let mut line = String::new();
    BufReader::new(file)
        .take(MAX_META_LINE)
        .read_line(&mut line)
        .ok()?;
    let v: serde_json::Value = serde_json::from_str(&line).ok()?;
    let cwd = v.get("payload")?.get("cwd")?.as_str()?;
    Some(PathBuf::from(cwd))
}

fn same_dir(a: &Path, wanted: &Path) -> bool {
    std::fs::canonicalize(a).is_ok_and(|a| a == wanted) || a == wanted
}

// ---- scrollback ----------------------------------------------------------------------------------

/// The terminal output as text turns: escape sequences gone, a line rewritten with a carriage return
/// shown as its last version, blank runs collapsed, then cut into chunks of about
/// [`CHUNK_LINES`] lines. The role is `assistant` (there is no terminal role); `source` tells.
///
/// Secrets are masked in the whole text *before* it is cut: a secret that spans lines (a private
/// key) would otherwise cross a chunk boundary, and the window may drop the chunk that holds its
/// first line. Returns the turns and how many secrets were masked.
fn scrollback_turns(bytes: &[u8]) -> (Vec<TranscriptTurn>, u32) {
    let text = strip_ansi(&String::from_utf8_lossy(bytes));
    let mut lines: Vec<&str> = Vec::new();
    let mut blank = true;
    for raw in text.split('\n') {
        let line = raw.trim_end_matches('\r');
        let line = line.rsplit('\r').next().unwrap_or(line).trim_end();
        if line.is_empty() && blank {
            continue; // leading blank lines, and a blank run after a blank line
        }
        blank = line.is_empty();
        lines.push(line);
    }
    while lines.last().is_some_and(|l| l.is_empty()) {
        lines.pop();
    }
    if lines.is_empty() {
        return (Vec::new(), 0);
    }
    let joined = lines.join("\n");
    let (masked, redactions) = redact(&joined);
    let lines: Vec<&str> = masked.split('\n').collect();
    let mut turns = Vec::new();
    let mut chunk: Vec<&str> = Vec::new();
    let mut chars = 0;
    let mut flush = |chunk: &mut Vec<&str>, chars: &mut usize| {
        if !chunk.is_empty() {
            turns.push(TranscriptTurn {
                role: TranscriptRole::Assistant,
                text: chunk.join("\n"),
                tool: None,
                at: None,
                approval: None,
                sub_agent: None,
            });
            chunk.clear();
            *chars = 0;
        }
    };
    for line in lines {
        if chunk.len() >= CHUNK_LINES || chars >= CHUNK_CHARS {
            flush(&mut chunk, &mut chars);
        }
        chars += line.chars().count() + 1;
        chunk.push(line);
    }
    flush(&mut chunk, &mut chars);
    (turns, redactions)
}

/// `text` without terminal control sequences: CSI (`ESC [ … final`), OSC / DCS / PM / APC strings
/// (up to BEL or `ESC \`), two-character escapes, and control characters other than newline, tab and
/// carriage return. A backspace erases the character before it.
fn strip_ansi(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '\x1b' => match chars.next() {
                Some('[') => {
                    for c in chars.by_ref() {
                        if ('@'..='~').contains(&c) {
                            break;
                        }
                    }
                }
                Some(']' | 'P' | '^' | '_') => {
                    while let Some(c) = chars.next() {
                        if c == '\x07' {
                            break;
                        }
                        if c == '\x1b' {
                            chars.next_if_eq(&'\\');
                            break;
                        }
                    }
                }
                // `ESC ( B` and the like: one more character belongs to it
                Some('(' | ')' | '*' | '+' | '#' | '%') => {
                    chars.next();
                }
                _ => {}
            },
            '\x08' => {
                if out.ends_with(|c| c != '\n' && c != '\r') {
                    out.pop();
                }
            }
            '\n' | '\t' | '\r' => out.push(c),
            c if c.is_control() => {}
            c => out.push(c),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use lotse_proto::{PermDecision, PermVia, UnixMs};

    #[test]
    fn ansi_sequences_are_stripped() {
        let raw = "a\x1b[1;31mred\x1b[0m \x1b]0;title\x07b \x1b]8;;http://x\x1b\\link\x1b]8;;\x1b\\ \x1b(Bc\x1b[?25l\x1b[2K\x07d";
        assert_eq!(strip_ansi(raw), "ared b link cd");
        assert_eq!(strip_ansi("abc\x08\x08x"), "ax");
        assert_eq!(strip_ansi("tab\there\r\nnext"), "tab\there\r\nnext");
        // a cut-off sequence at the end of the ring does not eat what came before it
        assert_eq!(strip_ansi("text\x1b[3"), "text");
        assert_eq!(strip_ansi("é ünï \u{1F600}"), "é ünï \u{1F600}");
    }

    #[test]
    fn scrollback_lines_are_cleaned_and_chunked() {
        let raw = "\r\n\r\nfirst\r\nprogress 1%\rprogress 99%\r\n\r\n\r\n\r\nlast  \r\n\r\n";
        let (turns, masked) = scrollback_turns(raw.as_bytes());
        assert_eq!(turns.len(), 1);
        assert_eq!(turns[0].text, "first\nprogress 99%\n\nlast");
        assert_eq!(masked, 0);
        assert!(scrollback_turns(b"").0.is_empty());
        assert!(scrollback_turns(b"\x1b[2J\r\n\r\n").0.is_empty());

        let many: String = (0..100).map(|i| format!("line {i}\n")).collect();
        let (turns, _) = scrollback_turns(many.as_bytes());
        assert_eq!(turns.len(), 3, "40 + 40 + 20 lines");
        assert!(turns[2].text.ends_with("line 99"));
        assert!(turns.iter().all(|t| t.role == TranscriptRole::Assistant));
    }

    #[test]
    fn a_private_key_across_chunks_is_masked_before_the_cut() {
        let mut lines: Vec<String> = (1..30).map(|i| format!("line {i}")).collect();
        lines.push("-----BEGIN RSA PRIVATE KEY-----".into());
        lines.extend((0..51).map(|i| format!("KEYBODY{i:02}{}", "aB3x".repeat(14))));
        lines.push("-----END RSA PRIVATE KEY-----".into());
        lines.push("after".into());
        let (turns, masked) = scrollback_turns((lines.join("\r\n") + "\r\n").as_bytes());
        assert_eq!(masked, 1);
        let all: Vec<&str> = turns.iter().map(|t| t.text.as_str()).collect();
        assert!(
            all.iter()
                .all(|t| !t.contains("KEYBODY") && !t.contains("END RSA"))
        );
        assert!(all.join("\n").ends_with("line 29\n[REDACTED]\nafter"));
        // the ring buffer may start inside the key: only its tail is left
        let tail = lines[60..].join("\n");
        let (turns, masked) = scrollback_turns(tail.as_bytes());
        assert_eq!(masked, 1);
        assert_eq!(turns.len(), 1);
        assert_eq!(turns[0].text, "[REDACTED]\nafter");
        // the same tail with a short last body line (padding)
        let mut tail: Vec<String> = lines[60..].to_vec();
        tail.insert(tail.len() - 2, "Ag==".into());
        let (turns, masked) = scrollback_turns(tail.join("\n").as_bytes());
        assert_eq!(masked, 1);
        assert_eq!(turns.len(), 1);
        assert_eq!(turns[0].text, "[REDACTED]\nafter");
    }

    fn event(
        id: &str,
        decision: Option<PermDecision>,
        asked: UnixMs,
        decided: Option<UnixMs>,
    ) -> PermEventRow {
        PermEventRow {
            session_id: SessionId::new("s"),
            prompt_id: id.into(),
            tool: "Bash".into(),
            summary: format!("cmd {id}"),
            asked_at: asked,
            decided_at: decided,
            decision,
            via: decision.map(|_| PermVia::Sidebar),
        }
    }

    fn at(ms: UnixMs) -> TranscriptTurn {
        TranscriptTurn {
            role: TranscriptRole::User,
            text: ms.to_string(),
            tool: None,
            at: Some(ms),
            approval: None,
            sub_agent: None,
        }
    }

    fn order(turns: &[TranscriptTurn]) -> Vec<String> {
        turns
            .iter()
            .map(|t| match &t.approval {
                Some(a) => a.command.clone(),
                None => t.text.clone(),
            })
            .collect()
    }

    #[test]
    fn approvals_go_in_by_the_time_they_were_decided() {
        let mut turns = vec![at(10), at(20), TranscriptTurn { at: None, ..at(0) }, at(30)];
        let events = [
            event("late", Some(PermDecision::Deny), 90, Some(100)),
            event("mid", Some(PermDecision::Allow), 12, Some(25)),
            event("early", Some(PermDecision::Always), 1, Some(5)),
            event("terminal", None, 15, Some(16)),
            event("open", None, 40, None),
        ];
        merge_approvals(&mut turns, &events);
        assert_eq!(
            order(&turns),
            [
                "Bash(cmd early)",
                "10",
                "20",
                "0",
                "Bash(cmd mid)",
                "30",
                "Bash(cmd late)"
            ]
        );
        // a decision without a time stamp falls back to when it was asked
        let mut turns = vec![at(10), at(20)];
        merge_approvals(
            &mut turns,
            &[event("x", Some(PermDecision::Allow), 15, None)],
        );
        assert_eq!(order(&turns), ["10", "Bash(cmd x)", "20"]);
    }

    #[test]
    fn rollouts_are_found_newest_first_and_only_for_the_folder() {
        let tmp = tempfile::tempdir().unwrap();
        let day = tmp.path().join("sessions/2026/11/03");
        std::fs::create_dir_all(&day).unwrap();
        let old_day = tmp.path().join("sessions/2026/10/30");
        std::fs::create_dir_all(&old_day).unwrap();
        let write = |dir: &Path, name: &str, cwd: Option<&str>| {
            let meta = match cwd {
                Some(cwd) => {
                    format!("{{\"type\":\"session_meta\",\"payload\":{{\"cwd\":\"{cwd}\"}}}}\n")
                }
                None => "{\"type\":\"session_meta\",\"payload\":{}}\n".into(),
            };
            std::fs::write(dir.join(name), meta).unwrap();
            dir.join(name)
        };
        let work = tmp.path().join("work");
        std::fs::create_dir_all(&work).unwrap();
        let work_str = work.to_string_lossy().into_owned();
        write(&day, "notes.txt", None);
        write(&day, "rollout-other.jsonl", Some("/elsewhere"));
        let sessions = tmp.path().join("sessions");
        assert_eq!(
            newest_rollout(&sessions, &work),
            None,
            "only another folder's"
        );
        let unknown = write(&day, "rollout-unknown.jsonl", None);
        assert_eq!(
            newest_rollout(&sessions, &work),
            Some(unknown.clone()),
            "fallback"
        );
        let mine = write(&old_day, "rollout-mine.jsonl", Some(&work_str));
        assert_eq!(
            newest_rollout(&sessions, &work),
            Some(mine),
            "the folder's wins"
        );
        assert_eq!(newest_rollout(&tmp.path().join("missing"), &work), None);
    }

    #[test]
    fn a_mirrored_sessions_folder_is_searched_and_a_linked_rollout_is_not() {
        // Lotse's Codex mirror links its `sessions` to the real one
        let tmp = tempfile::tempdir().unwrap();
        let real = tmp.path().join("real/sessions/2026/11/03");
        std::fs::create_dir_all(&real).unwrap();
        let mirror = tmp.path().join("mirror");
        std::fs::create_dir_all(&mirror).unwrap();
        std::os::unix::fs::symlink(tmp.path().join("real/sessions"), mirror.join("sessions"))
            .unwrap();
        let work = tmp.path().join("work");
        std::fs::create_dir_all(&work).unwrap();
        let meta = |cwd: &Path| {
            format!(
                "{{\"type\":\"session_meta\",\"payload\":{{\"cwd\":\"{}\"}}}}\n",
                cwd.display()
            )
        };
        std::fs::write(
            real.join("rollout-other.jsonl"),
            meta(Path::new("/elsewhere")),
        )
        .unwrap();
        let sessions = mirror.join("sessions");
        assert_eq!(
            newest_rollout(&sessions, &work),
            None,
            "the other folder's rollout is read through the mirror, not taken as unknown"
        );
        let mine = real.join("rollout-mine.jsonl");
        std::fs::write(&mine, meta(&work)).unwrap();
        assert_eq!(
            newest_rollout(&sessions, &work),
            Some(sessions.join("2026/11/03/rollout-mine.jsonl"))
        );

        // a rollout that is itself a link is never taken
        std::fs::remove_file(&mine).unwrap();
        let target = tmp.path().join("target.txt");
        std::fs::write(&target, meta(&work)).unwrap();
        std::os::unix::fs::symlink(&target, &mine).unwrap();
        assert_eq!(newest_rollout(&sessions, &work), None);
    }
}
