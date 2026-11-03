//! Beta (wp06): time, tokens and spend of one session: `get_usage`.
//!
//! - **Time** comes from the history the recorder keeps ([`crate::history`]): `total` is the age of
//!   the session, `working` the time its own agent was in `run`, `waiting` in `perm`. Open spans
//!   run to now. Lanes: the session's own agent first, then one per sub-agent (named from the
//!   session's current sub-agent list, else by id).
//! - **Tokens** are summed over the whole Claude Code transcript the session's hooks announced
//!   (same rules as [`crate::usage`]: a message counts once, the latest line of it wins). Other
//!   agents have no parseable transcript: tokens 0, `priced: false`, time still filled.
//! - **Spend** is today's (local day) cost: this session, its project's live sessions, and every
//!   transcript the daemon remembers. The lead lane's `cost_micros` is the session's whole cost;
//!   sub-agent lanes carry 0 (the transcript does not say which sub-agent spent what).
//! - **Prices** come from [`crate::usage::price_for`]. A message of a model without a price makes
//!   `priced` false; the spend then covers the priced messages only.
//!
//! **Unverified:** each call reads the session's transcript from the start (and today's other
//! transcripts), so a very large file costs a read per call; there is no cache yet.

use crate::db::SpanRow;
use crate::session::now_ms;
use crate::state::{State, unavailable};
use crate::transcript::{parse_iso_utc_ms, validate_path};
use crate::usage::{DayBounds, price_for};
use lotse_proto::{
    AgentKind, ErrorCode, LaneKind, ProtoError, Request, Response, Session, Status, Usage,
    UsageLane, UsageSpan, UsageSpend, UsageTokens,
};
use serde_json::Value;
use std::collections::{HashMap, HashSet};
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, UNIX_EPOCH};

fn error(code: ErrorCode, message: &str) -> Response {
    Response::Error(ProtoError {
        code,
        message: message.to_owned(),
    })
}

pub async fn handle(state: &Arc<State>, req: Request) -> Response {
    let Request::GetUsage { session_id } = req else {
        return unavailable(req.method());
    };
    let Some(handle) = state.session(&session_id) else {
        return error(ErrorCode::NotFound, "session not found");
    };
    let session = handle.snapshot();
    let transcript = handle
        .transcript()
        .filter(|(agent, _)| *agent == AgentKind::ClaudeCode)
        .map(|(_, path)| path);
    let state = state.clone();
    match tokio::task::spawn_blocking(move || compute(&state, &session, transcript.as_deref()))
        .await
    {
        Ok(usage) => Response::Usage(usage),
        Err(e) => {
            tracing::error!("get_usage: {e}");
            error(ErrorCode::Internal, "internal error")
        }
    }
}

fn compute(state: &State, session: &Session, transcript: Option<&Path>) -> Usage {
    let now = now_ms();
    let day = DayBounds::local(now);
    let db = state.db();

    let rows = db.list_spans(&session.id).unwrap_or_else(|e| {
        tracing::warn!("get_usage: reading spans of {}: {e}", session.id);
        Vec::new()
    });
    let names: HashMap<&str, &str> = session
        .sub_agents
        .iter()
        .map(|a| (a.id.as_str(), a.name.as_str()))
        .collect();
    let mut lanes = lanes(&rows, session.agent.label(), &names, now);
    let (working_ms, waiting_ms) = lanes.first().map_or((0, 0), |l| time_split(&l.spans));

    let own = transcript.and_then(|p| match scan(p, day) {
        Ok(s) => Some(s),
        Err(e) => {
            tracing::debug!("get_usage: transcript of {}: {e}", session.id);
            None
        }
    });
    if let Some(path) = transcript
        && let Err(e) = db.tag_transcript(&path.to_string_lossy(), &session.id)
    {
        tracing::warn!("get_usage: tagging a transcript: {e}");
    }

    // today's spend: the session, its project (live sessions), everything the daemon remembers
    let mut seen: HashSet<PathBuf> = transcript.map(Path::to_path_buf).into_iter().collect();
    let session_today = own.as_ref().map_or(0, |s| s.today.micros_priced());
    let mut project_today = session_today;
    let siblings = db
        .list_sessions()
        .unwrap_or_default()
        .into_iter()
        .filter(|r| r.project_id == session.project_id && r.id != session.id);
    for row in siblings {
        let Some((AgentKind::ClaudeCode, path)) =
            state.session(&row.id).and_then(|h| h.transcript())
        else {
            continue;
        };
        if seen.insert(path.clone()) {
            project_today += today_of(&path, day);
        }
    }
    let mut all_today = project_today;
    if let Some(home) = state.config().home.as_deref() {
        for raw in db.all_transcripts().unwrap_or_default() {
            if let Some(path) = validate_path(&raw, home)
                && seen.insert(path.clone())
            {
                all_today += today_of(&path, day);
            }
        }
    }

    if let Some(lead) = lanes.first_mut() {
        lead.cost_micros = own.as_ref().map_or(0, |s| s.all.micros_priced());
    }
    Usage {
        started_at: session.created_at,
        total_ms: now.saturating_sub(session.created_at),
        working_ms,
        waiting_ms,
        lanes,
        tokens: own.as_ref().map(|s| s.tokens).unwrap_or_default(),
        spend: UsageSpend {
            session: session_today,
            project: project_today,
            all: all_today,
        },
        model: own.as_ref().and_then(|s| s.model.clone()),
        priced: own.as_ref().is_some_and(|s| s.all.micros().is_some()),
    }
}

// time

/// The session's own lane first (always there), then one lane per sub-agent in order of appearance.
fn lanes(
    rows: &[SpanRow],
    lead_name: &str,
    sub_agent_names: &HashMap<&str, &str>,
    now: u64,
) -> Vec<UsageLane> {
    let span = |r: &SpanRow| UsageSpan {
        status: r.status,
        from: r.from_ms,
        to: r.to_ms.unwrap_or_else(|| now.max(r.from_ms)),
    };
    let mut lanes = vec![UsageLane {
        name: lead_name.to_owned(),
        kind: LaneKind::Lead,
        spans: rows
            .iter()
            .filter(|r| r.sub_agent_id.is_none())
            .map(span)
            .collect(),
        cost_micros: 0,
    }];
    let mut lane_of: HashMap<&str, usize> = HashMap::new();
    for row in rows {
        let Some(id) = row.sub_agent_id.as_deref() else {
            continue;
        };
        let at = *lane_of.entry(id).or_insert_with(|| {
            lanes.push(UsageLane {
                name: sub_agent_names.get(id).copied().unwrap_or(id).to_owned(),
                kind: LaneKind::SubAgent,
                spans: Vec::new(),
                cost_micros: 0,
            });
            lanes.len() - 1
        });
        lanes[at].spans.push(span(row));
    }
    lanes
}

/// `(working, waiting)`: milliseconds in `run` and in `perm`.
fn time_split(spans: &[UsageSpan]) -> (u64, u64) {
    let sum = |status| {
        spans
            .iter()
            .filter(|s| s.status == status)
            .map(|s| s.to.saturating_sub(s.from))
            .sum()
    };
    (sum(Status::Run), sum(Status::Perm))
}

// tokens and cost

/// Cost in millionths of a dollar times a million (divide once, at the end: no rounding drift).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
struct Cost {
    numerator: u128,
    /// A message of a model without a price took part.
    unpriced: bool,
}

impl Cost {
    /// `None` when any message has no price: the figure would be partial.
    fn micros(self) -> Option<u64> {
        (!self.unpriced).then(|| self.micros_priced())
    }

    /// The cost of the messages that have a price.
    fn micros_priced(self) -> u64 {
        (self.numerator / 1_000_000) as u64
    }
}

#[derive(Debug, Default)]
struct Scan {
    tokens: UsageTokens,
    all: Cost,
    today: Cost,
    /// Model of the newest message.
    model: Option<String>,
}

struct Message {
    model: String,
    tokens: UsageTokens,
    at: u64,
}

impl Message {
    fn cost(&self) -> Cost {
        match price_for(&self.model) {
            // cache reads cost a tenth of the input price, 5-minute cache writes 1.25 times (the
            // same rules as `usage`)
            Some(p) => Cost {
                numerator: u128::from(self.tokens.input) * u128::from(p.input)
                    + u128::from(self.tokens.output) * u128::from(p.output)
                    + u128::from(self.tokens.cache_read) * u128::from(p.input / 10)
                    + u128::from(self.tokens.cache_write) * u128::from(p.input / 4 * 5),
                unpriced: false,
            },
            None => Cost {
                numerator: 0,
                unpriced: true,
            },
        }
    }
}

/// Today's priced cost of a transcript; a file last written before today (or unreadable) is 0.
fn today_of(path: &Path, day: DayBounds) -> u64 {
    let day_start = UNIX_EPOCH + Duration::from_millis(day.start_ms);
    let written_today = std::fs::metadata(path)
        .and_then(|m| m.modified())
        .is_ok_and(|m| m >= day_start);
    if !written_today {
        return 0;
    }
    scan(path, day).map_or(0, |s| s.today.micros_priced())
}

/// Sum a whole transcript; `today` counts the messages of `day` only.
fn scan(path: &Path, day: DayBounds) -> std::io::Result<Scan> {
    let mut reader = BufReader::new(std::fs::File::open(path)?);
    let mut messages: HashMap<String, Message> = HashMap::new();
    let mut line = Vec::new();
    loop {
        line.clear();
        if reader.read_until(b'\n', &mut line)? == 0 {
            break;
        }
        if let Some((key, message)) = parse_line(&line) {
            messages.insert(key, message);
        }
    }
    let mut out = Scan::default();
    let mut newest = 0;
    for m in messages.values() {
        out.tokens.input += m.tokens.input;
        out.tokens.output += m.tokens.output;
        out.tokens.cache_read += m.tokens.cache_read;
        out.tokens.cache_write += m.tokens.cache_write;
        let cost = m.cost();
        out.all.numerator += cost.numerator;
        out.all.unpriced |= cost.unpriced;
        if (day.start_ms..day.end_ms).contains(&m.at) {
            out.today.numerator += cost.numerator;
            out.today.unpriced |= cost.unpriced;
        }
        if m.at >= newest {
            newest = m.at;
            out.model = Some(m.model.clone());
        }
    }
    Ok(out)
}

/// `(message key, message)` of an assistant line that carries token counts. One message is written
/// as several lines (one per content block), each repeating its `usage`: the key joins them.
fn parse_line(line: &[u8]) -> Option<(String, Message)> {
    // most lines (user text, tool results) have no usage: skip them without parsing
    if !line.windows(7).any(|w| w == b"\"usage\"") {
        return None;
    }
    let v: Value = serde_json::from_slice(line).ok()?;
    if v.get("type")?.as_str()? != "assistant" {
        return None;
    }
    let at = parse_iso_utc_ms(v.get("timestamp")?.as_str()?)?;
    let msg = v.get("message")?;
    let usage = msg.get("usage")?;
    let n = |k: &str| usage.get(k).and_then(Value::as_u64).unwrap_or(0);
    let tokens = UsageTokens {
        input: n("input_tokens"),
        cache_read: n("cache_read_input_tokens"),
        cache_write: n("cache_creation_input_tokens"),
        output: n("output_tokens"),
    };
    if tokens.input + tokens.cache_read + tokens.cache_write + tokens.output == 0 {
        return None; // e.g. the `<synthetic>` placeholder messages
    }
    let key = match (
        msg.get("id").and_then(Value::as_str),
        v.get("requestId").and_then(Value::as_str),
    ) {
        (Some(id), Some(req)) => format!("{id}:{req}"),
        (Some(id), None) => id.to_owned(),
        _ => v.get("uuid")?.as_str()?.to_owned(),
    };
    let model = msg
        .get("model")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_owned();
    Some((key, Message { model, tokens, at }))
}

#[cfg(test)]
mod tests {
    use super::*;

    // 2026-11-02 00:00:00Z .. 2026-11-03 00:00:00Z
    const DAY: DayBounds = DayBounds {
        start_ms: 1_793_577_600_000,
        end_ms: 1_793_664_000_000,
    };

    fn line(id: &str, model: &str, ts: &str, usage: (u64, u64, u64, u64)) -> String {
        format!(
            "{{\"type\":\"assistant\",\"timestamp\":\"{ts}\",\"requestId\":\"r-{id}\",\"message\":{{\"id\":\"{id}\",\"model\":\"{model}\",\"usage\":{{\"input_tokens\":{},\"output_tokens\":{},\"cache_read_input_tokens\":{},\"cache_creation_input_tokens\":{}}}}}}}\n",
            usage.0, usage.1, usage.2, usage.3
        )
    }

    fn scan_text(text: &str) -> Scan {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("s.jsonl");
        std::fs::write(&path, text).unwrap();
        scan(&path, DAY).unwrap()
    }

    #[test]
    fn an_unknown_model_has_no_cost() {
        let mut text = line(
            "a",
            "claude-sonnet-4-5",
            "2026-11-02T10:00:00.000Z",
            (1_000_000, 0, 0, 0),
        );
        let s = scan_text(&text);
        assert_eq!(s.all.micros(), Some(3_000_000));

        text += &line(
            "b",
            "claude-fable-9-9",
            "2026-11-02T10:00:01.000Z",
            (5, 0, 0, 0),
        );
        let s = scan_text(&text);
        assert_eq!(s.all.micros(), None, "no price for one message: no cost");
        assert_eq!(s.all.micros_priced(), 3_000_000, "the priced part stays");
        assert_eq!(s.tokens.input, 1_000_005);
        assert_eq!(s.model.as_deref(), Some("claude-fable-9-9"));
    }

    #[test]
    fn today_is_the_day_and_everything_is_the_file() {
        let mut text = line(
            "old",
            "claude-sonnet-4-5",
            "2026-11-01T23:59:59.999Z",
            (1_000_000, 0, 0, 0),
        );
        text += &line(
            "new",
            "claude-sonnet-4-5",
            "2026-11-02T00:00:00.000Z",
            (1_000_000, 0, 0, 0),
        );
        text += &line(
            "next",
            "claude-sonnet-4-5",
            "2026-11-03T00:00:00.000Z",
            (1_000_000, 0, 0, 0),
        );
        let s = scan_text(&text);
        assert_eq!(s.tokens.input, 3_000_000);
        assert_eq!(s.all.micros(), Some(9_000_000));
        assert_eq!(s.today.micros(), Some(3_000_000));
    }

    #[test]
    fn a_message_written_as_several_lines_counts_once_with_its_latest_usage() {
        let ts = "2026-11-02T10:00:00.000Z";
        let mut text = line("m1", "claude-sonnet-4-5", ts, (100, 1, 0, 0));
        text += &line("m1", "claude-sonnet-4-5", ts, (100, 50, 0, 0));
        text += "not json at all\n";
        text += &line("m2", "claude-sonnet-4-5", ts, (0, 0, 0, 0)); // no tokens: not a message
        let s = scan_text(&text);
        assert_eq!((s.tokens.input, s.tokens.output), (100, 50));
    }

    #[test]
    fn cache_tokens_are_priced_like_the_status_bar_prices_them() {
        // Opus 4.5: $5 in, $25 out, cache read $0.50, cache write $6.25 per million
        let s = scan_text(&line(
            "m1",
            "claude-opus-4-5-20251101",
            "2026-11-02T10:00:00.000Z",
            (1_000_000, 100_000, 2_000_000, 400_000),
        ));
        assert_eq!(s.all.micros(), Some(11_000_000));
    }

    fn span(status: Status, from: u64, to: u64) -> UsageSpan {
        UsageSpan { status, from, to }
    }

    #[test]
    fn time_is_split_into_working_and_waiting() {
        let spans = [
            span(Status::Run, 0, 10_000),
            span(Status::Perm, 10_000, 14_000),
            span(Status::Run, 14_000, 20_000),
            span(Status::Idle, 20_000, 90_000),
        ];
        assert_eq!(time_split(&spans), (16_000, 4_000));
        assert_eq!(time_split(&[]), (0, 0));
    }

    fn row(sub: Option<&str>, status: Status, from: u64, to: Option<u64>) -> SpanRow {
        SpanRow {
            sub_agent_id: sub.map(str::to_owned),
            status,
            from_ms: from,
            to_ms: to,
        }
    }

    #[test]
    fn lanes_put_the_agent_first_and_open_spans_run_to_now() {
        let rows = [
            row(None, Status::Run, 100, Some(200)),
            row(Some("a1"), Status::Run, 120, Some(180)),
            row(None, Status::Perm, 200, None),
            row(Some("a1"), Status::Done, 180, None),
            row(Some("a2"), Status::Run, 150, None),
        ];
        let names = HashMap::from([("a1", "Explore")]);
        let lanes = lanes(&rows, "Claude Code", &names, 500);
        let shape: Vec<_> = lanes
            .iter()
            .map(|l| (l.name.as_str(), l.kind, l.spans.len()))
            .collect();
        assert_eq!(
            shape,
            [
                ("Claude Code", LaneKind::Lead, 2),
                ("Explore", LaneKind::SubAgent, 2),
                ("a2", LaneKind::SubAgent, 1),
            ]
        );
        assert_eq!(lanes[0].spans[1], span(Status::Perm, 200, 500));
        assert_eq!(time_split(&lanes[0].spans), (100, 300));
    }

    #[test]
    fn a_session_without_history_still_has_its_lane() {
        let lanes = lanes(&[], "Codex", &HashMap::new(), 500);
        assert_eq!(lanes.len(), 1);
        assert!(lanes[0].spans.is_empty());
    }
}
