//! Beta (wp07): `export_transcript`: the session's whole transcript as a document, secrets masked.
//!
//! The transcript comes from [`crate::transcript_sources`] (up to its last 20 MiB, approvals merged
//! in, every text masked by [`crate::redact`]); this module renders it as Markdown, JSON Lines (one
//! `TranscriptTurn` per line) or plain text.
//!
//! The answer is one frame, and a frame holds at most 16 MiB (`MAX_FRAME_LEN`, frozen with the
//! protocol): when the document would not fit, the oldest turns are left out (Markdown and text say
//! so; JSON Lines has no room for a note) rather than failing the whole export.
//!
//! `file_name` is `<project>-<session>-<yyyymmdd-hhmm>.<ext>` in local time, with the project and
//! session names reduced to letters, digits and `-` so the name is safe on any file system.

use crate::redact::redact;
use crate::session::now_ms;
use crate::state::{State, unavailable};
use crate::transcript_sources::{self, Scope, error};
use lotse_proto::{
    ErrorCode, Export, ExportFormat, PermDecision, PermVia, Request, Response, SessionId,
    ToolStatus, TranscriptRole, TranscriptSource, TranscriptTurn, UnixMs,
};
use std::sync::Arc;

/// The encoded `content` may take this much of a frame (16 MiB, minus the envelope).
const MAX_ENCODED: usize = 15 * 1024 * 1024;
/// Longest project or session name in a file name, in characters.
const MAX_NAME: usize = 40;

pub async fn handle(state: &Arc<State>, req: Request) -> Response {
    match req {
        Request::ExportTranscript { session_id, format } => {
            export(state, &session_id, format).await
        }
        other => unavailable(other.method()),
    }
}

async fn export(state: &Arc<State>, id: &SessionId, format: ExportFormat) -> Response {
    let Some(h) = state.session(id) else {
        return error(ErrorCode::NotFound, "session not found");
    };
    let meta = h.snapshot();
    let project = match state.db().get_project(&meta.project_id) {
        Ok(Some(p)) => p.name,
        _ => String::new(),
    };
    let collected = match transcript_sources::collect(state, &h, Scope::Full).await {
        Ok(c) => c,
        Err(e) => return error(ErrorCode::Unavailable, format!("transcript: {e}")),
    };
    if !collected.transcript.available {
        return error(
            ErrorCode::Unavailable,
            "the agent has not reported a transcript yet",
        );
    }
    let doc = Doc {
        project,
        session: meta.title,
        agent: meta.agent.label().to_owned(),
        model: collected.model,
        source: collected.transcript.source,
        turns: collected.transcript.turns,
        cut: collected.transcript.truncated,
        exported_at: now_ms(),
        redactions: collected.redactions,
    };
    match tokio::task::spawn_blocking(move || render(doc, format)).await {
        Ok(export) => Response::Export(export),
        Err(e) => {
            tracing::error!("export: {e}");
            error(ErrorCode::Internal, "internal error")
        }
    }
}

struct Doc {
    project: String,
    session: String,
    agent: String,
    model: Option<String>,
    source: TranscriptSource,
    turns: Vec<TranscriptTurn>,
    /// The transcript file is longer than what was read.
    cut: bool,
    exported_at: UnixMs,
    /// Already masked in the turns.
    redactions: u32,
}

fn render(mut doc: Doc, format: ExportFormat) -> Export {
    // the names are in the document and in the file name: mask them first
    let (project, a) = redact(&doc.project);
    let (session, b) = redact(&doc.session);
    let (project, session) = (project.into_owned(), session.into_owned());
    doc.redactions += a + b;

    let (ext, header, chunks) = match format {
        ExportFormat::Markdown => (
            "md",
            markdown_header(&doc, &project, &session),
            doc.turns.iter().map(markdown_turn).collect::<Vec<_>>(),
        ),
        ExportFormat::JsonLines => (
            "jsonl",
            String::new(),
            doc.turns
                .iter()
                .map(|t| serde_json::to_string(t).unwrap_or_default() + "\n")
                .collect(),
        ),
        ExportFormat::Text => (
            "txt",
            String::new(),
            doc.turns.iter().map(text_turn).collect(),
        ),
    };
    let (kept, dropped) = fit(chunks, MAX_ENCODED.saturating_sub(json_len(&header)));
    let mut content = header;
    if dropped || doc.cut {
        match format {
            ExportFormat::Markdown => {
                content.push_str("_Older turns omitted to fit the size limit._\n\n");
            }
            ExportFormat::Text => content.push_str("(older turns omitted to fit the size limit)\n"),
            ExportFormat::JsonLines => {}
        }
    }
    content.extend(kept.iter().map(String::as_str));
    Export {
        file_name: format!(
            "{}-{}-{}.{ext}",
            slug(&project, "project"),
            slug(&session, "session"),
            local_stamp(doc.exported_at)
        ),
        content,
        redactions: doc.redactions,
    }
}

/// The newest of `chunks` whose JSON-encoded size stays within `budget`, and whether older ones
/// were dropped.
fn fit(chunks: Vec<String>, budget: usize) -> (Vec<String>, bool) {
    let mut size = 0;
    let first = chunks
        .iter()
        .rposition(|c| {
            size += json_len(c);
            size > budget
        })
        .map_or(0, |i| i + 1);
    let dropped = first > 0;
    (chunks.into_iter().skip(first).collect(), dropped)
}

/// Bytes `s` takes inside a JSON string (quotes not counted).
fn json_len(s: &str) -> usize {
    s.chars()
        .map(|c| match c {
            '"' | '\\' | '\n' | '\r' | '\t' | '\x08' | '\x0c' => 2,
            c if c < ' ' => 6,
            c => c.len_utf8(),
        })
        .sum()
}

// ---- Markdown ------------------------------------------------------------------------------------

fn markdown_header(doc: &Doc, project: &str, session: &str) -> String {
    let mut h = format!(
        "# {session}\n\n- Project: {project}\n- Agent: {}\n",
        doc.agent
    );
    if let Some(model) = &doc.model {
        h.push_str(&format!("- Model: {model}\n"));
    }
    h.push_str(&format!(
        "- Source: {}\n- Exported: {}\n\n---\n\n",
        source_label(doc.source),
        iso(doc.exported_at)
    ));
    h
}

fn markdown_turn(t: &TranscriptTurn) -> String {
    let when = t.at.map_or(String::new(), |at| format!(" — {}", iso(at)));
    match (&t.role, &t.tool, &t.approval) {
        (TranscriptRole::Tool, Some(tool), _) => {
            let mut head = format!("### Tool: {}", tool.name);
            if let Some(status) = tool.status {
                head.push_str(&format!(" ({})", status_label(status)));
            }
            if let Some(sub) = &t.sub_agent {
                head.push_str(&format!(" [sub-agent {sub}]"));
            }
            let mut out = format!("{head}{when}\n\n{}\n\n", fence(&tool.summary));
            if let Some(result) = &tool.result {
                out.push_str(&format!("Output:\n\n{}\n\n", fence(result)));
            }
            out
        }
        (TranscriptRole::Approval, _, Some(a)) => format!(
            "### Approval: {}{}{when}\n\n{}\n\n",
            decision_label(a.decision),
            a.via
                .map_or(String::new(), |v| format!(" via {}", via_label(v))),
            fence(&a.command)
        ),
        _ => format!("### {}{when}\n\n{}\n\n", role_title(t.role), t.text),
    }
}

/// `text` in a code fence that its own backticks cannot close.
fn fence(text: &str) -> String {
    let longest = text
        .split(|c| c != '`')
        .map(str::len)
        .max()
        .unwrap_or_default();
    let ticks = "`".repeat((longest + 1).max(3));
    format!("{ticks}\n{text}\n{ticks}")
}

// ---- plain text ----------------------------------------------------------------------------------

fn text_turn(t: &TranscriptTurn) -> String {
    let when = t.at.map_or(String::new(), |at| format!("[{}] ", iso(at)));
    match (&t.role, &t.tool, &t.approval) {
        (TranscriptRole::Tool, Some(tool), _) => {
            let status = tool
                .status
                .map_or(String::new(), |s| format!(" ({})", status_label(s)));
            let sub = t
                .sub_agent
                .as_ref()
                .map_or(String::new(), |s| format!(" [sub-agent {s}]"));
            let mut out = format!("{when}tool {}{status}: {}{sub}\n", tool.name, tool.summary);
            for line in tool.result.iter().flat_map(|r| r.lines()) {
                out.push_str(&format!("  > {line}\n"));
            }
            out
        }
        (TranscriptRole::Approval, _, Some(a)) => format!(
            "{when}approval {}{}: {}\n",
            decision_label(a.decision),
            a.via
                .map_or(String::new(), |v| format!(" via {}", via_label(v))),
            a.command
        ),
        _ => format!("{when}{}: {}\n", role_label(t.role), t.text),
    }
}

// ---- labels, names and times ---------------------------------------------------------------------

fn role_label(role: TranscriptRole) -> &'static str {
    match role {
        TranscriptRole::User => "user",
        TranscriptRole::Assistant => "assistant",
        TranscriptRole::Tool => "tool",
        TranscriptRole::Approval => "approval",
    }
}

fn role_title(role: TranscriptRole) -> &'static str {
    match role {
        TranscriptRole::User => "User",
        TranscriptRole::Assistant => "Assistant",
        TranscriptRole::Tool => "Tool",
        TranscriptRole::Approval => "Approval",
    }
}

fn status_label(status: ToolStatus) -> &'static str {
    match status {
        ToolStatus::Run => "run",
        ToolStatus::Done => "done",
        ToolStatus::Fail => "fail",
    }
}

fn decision_label(decision: PermDecision) -> &'static str {
    match decision {
        PermDecision::Allow => "allow",
        PermDecision::Always => "always",
        PermDecision::Deny => "deny",
    }
}

fn via_label(via: PermVia) -> &'static str {
    match via {
        PermVia::Sidebar => "sidebar",
        PermVia::Notification => "notification",
        PermVia::Palette => "palette",
    }
}

fn source_label(source: TranscriptSource) -> &'static str {
    match source {
        TranscriptSource::Claude => "Claude Code transcript",
        TranscriptSource::Codex => "Codex rollout",
        TranscriptSource::OpenCode => "OpenCode scrollback",
        TranscriptSource::Scrollback => "terminal scrollback",
    }
}

/// Letters, digits and `_` of `name`, other runs as one `-`, at most [`MAX_NAME`] characters;
/// `fallback` when nothing is left.
fn slug(name: &str, fallback: &str) -> String {
    let mut out = String::new();
    for c in name.chars() {
        if c.is_alphanumeric() || c == '_' {
            out.push(c);
        } else if !out.ends_with('-') {
            out.push('-');
        }
    }
    let out: String = out.trim_matches('-').chars().take(MAX_NAME).collect();
    let out = out.trim_end_matches('-');
    if out.is_empty() {
        fallback.to_owned()
    } else {
        out.to_owned()
    }
}

/// `2026-11-03 09:00:00Z`: UTC, as the agents write their timestamps.
fn iso(ms: UnixMs) -> String {
    let (y, mo, d, h, mi, s) = utc_parts(ms);
    format!("{y:04}-{mo:02}-{d:02} {h:02}:{mi:02}:{s:02}Z")
}

/// Year, month, day, hour, minute, second in UTC (days from civil, Howard Hinnant).
fn utc_parts(ms: UnixMs) -> (i64, u32, u32, u32, u32, u32) {
    let secs = i64::try_from(ms / 1000).unwrap_or(i64::MAX / 2);
    let (days, rem) = (secs.div_euclid(86_400), secs.rem_euclid(86_400));
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    (
        y,
        m as u32,
        d as u32,
        (rem / 3_600) as u32,
        (rem % 3_600 / 60) as u32,
        (rem % 60) as u32,
    )
}

/// `yyyymmdd-hhmm` in local time (UTC when the time zone cannot be read).
#[allow(unsafe_code)]
fn local_stamp(ms: UnixMs) -> String {
    use nix::libc;
    let secs = (ms / 1000) as libc::time_t;
    // SAFETY: `tm` is plain data owned here; `localtime_r` only writes it (and the C library's
    // time-zone state, which it initialises itself).
    let tm = unsafe {
        let mut tm: libc::tm = std::mem::zeroed();
        (!libc::localtime_r(&secs, &mut tm).is_null()).then_some(tm)
    };
    match tm {
        Some(tm) => format!(
            "{:04}{:02}{:02}-{:02}{:02}",
            tm.tm_year + 1900,
            tm.tm_mon + 1,
            tm.tm_mday,
            tm.tm_hour,
            tm.tm_min
        ),
        None => {
            let (y, mo, d, h, mi, _) = utc_parts(ms);
            format!("{y:04}{mo:02}{d:02}-{h:02}{mi:02}")
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use lotse_proto::{TranscriptApproval, TranscriptTool};

    #[test]
    fn times_are_utc_and_the_stamp_has_its_shape() {
        assert_eq!(iso(0), "1970-01-01 00:00:00Z");
        assert_eq!(iso(1_793_696_400_000), "2026-11-03 09:00:00Z");
        assert_eq!(iso(1_709_251_199_999), "2024-02-29 23:59:59Z");
        let stamp = local_stamp(1_793_696_400_000);
        assert_eq!(stamp.len(), 13, "{stamp}");
        assert_eq!(&stamp[8..9], "-");
        assert!(
            stamp.bytes().filter(u8::is_ascii_digit).count() == 12,
            "{stamp}"
        );
    }

    #[test]
    fn names_become_safe_file_name_parts() {
        assert_eq!(slug("Round cents", "x"), "Round-cents");
        assert_eq!(slug("  ../../etc/passwd  ", "x"), "etc-passwd");
        assert_eq!(slug("a/b\\c:d*e?f", "x"), "a-b-c-d-e-f");
        assert_eq!(slug("praxis_web 2", "x"), "praxis_web-2");
        assert_eq!(slug("日本語 テスト", "x"), "日本語-テスト");
        assert_eq!(slug("", "session"), "session");
        assert_eq!(slug("///", "project"), "project");
        assert_eq!(slug(&"a".repeat(100), "x").chars().count(), MAX_NAME);
        assert!(!slug("a.b", "x").contains('.'));
    }

    #[test]
    fn the_newest_chunks_that_fit_are_kept() {
        let chunks: Vec<String> = ["aaaa", "bb\n", "cccc", "dd"].map(String::from).to_vec();
        // "bb\n" is 4 bytes encoded
        assert_eq!(fit(chunks.clone(), 100), (chunks.clone(), false));
        assert_eq!(fit(chunks.clone(), 10), (chunks[1..].to_vec(), true));
        assert_eq!(fit(chunks.clone(), 9), (chunks[2..].to_vec(), true));
        assert_eq!(fit(chunks.clone(), 5), (chunks[3..].to_vec(), true));
        assert_eq!(fit(chunks, 1), (Vec::new(), true));
        assert_eq!(json_len("a\"b\\c\n\u{1}é"), 1 + 2 + 1 + 2 + 1 + 2 + 6 + 2);
    }

    #[test]
    fn fences_cannot_be_closed_from_inside() {
        assert_eq!(fence("ls"), "```\nls\n```");
        assert_eq!(fence("a ```b``` c"), "````\na ```b``` c\n````");
        assert_eq!(fence("``````"), "```````\n``````\n```````");
    }

    #[test]
    fn approvals_are_rendered_with_decision_and_source() {
        let turn = TranscriptTurn {
            role: TranscriptRole::Approval,
            text: String::new(),
            tool: None,
            at: Some(1_793_696_415_000),
            approval: Some(TranscriptApproval {
                decision: PermDecision::Deny,
                via: Some(PermVia::Notification),
                command: "Bash(rm -rf build)".into(),
            }),
            sub_agent: None,
        };
        assert_eq!(
            text_turn(&turn),
            "[2026-11-03 09:00:15Z] approval deny via notification: Bash(rm -rf build)\n"
        );
        assert_eq!(
            markdown_turn(&turn),
            "### Approval: deny via notification — 2026-11-03 09:00:15Z\n\n```\nBash(rm -rf build)\n```\n\n"
        );
        let tool = TranscriptTurn {
            role: TranscriptRole::Tool,
            tool: Some(TranscriptTool {
                name: "Bash".into(),
                summary: "ls".into(),
                status: None,
                result: None,
            }),
            approval: None,
            at: None,
            ..turn
        };
        assert_eq!(text_turn(&tool), "tool Bash: ls\n");
        assert_eq!(markdown_turn(&tool), "### Tool: Bash\n\n```\nls\n```\n\n");
    }
}
