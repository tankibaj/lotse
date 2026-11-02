//! Claude Code hook adapter: maps hook events to Lotse status. Source: [`DetectSource::Hook`].
//!
//! # Where the events come from
//!
//! `lotse-hook` (run by the plugin `lotsed` injects into every pane through `CLAUDE_CODE_PLUGIN_DIRS`, or
//! by the hooks the optional global install puts into `~/.claude/settings.json`; see `integrations/claude`)
//! forwards each hook payload verbatim as `Request::HookEvent`; the daemon wraps it in a [`HookObservation`].
//!
//! Event names and payload fields follow the Claude Code hooks reference:
//! <https://code.claude.com/docs/en/hooks> (older location:
//! <https://docs.anthropic.com/en/docs/claude-code/hooks>).
//!
//! **Unverified:** web access was unavailable when this was written, so the names below were
//! cross-checked against local sources only (Orca's and herdr's Claude hook integrations under
//! `/Users/naim/Repos/research/`, Orca's per-release event enum fixture), not against the live docs.
//! Re-check this list against the URL above before release. The adapter reads only:
//! `hook_event_name` (all events), `agent_id` + `agent_type` (sub-agent contexts: `SubagentStart`,
//! `SubagentStop`, and tool/permission events fired inside a sub-agent), `source` (`SessionStart`),
//! `tool_name`, `tool_use_id` and `tool_input` (tool events). Every other field is ignored, and an
//! unknown event name or a missing field never changes the status.
//!
//! # Status table
//!
//! The lead (the session's main agent) and each sub-agent are tracked separately. The reported
//! status is derived: lead `Perm`, else any sub-agent `Perm`, else lead `Fail`, else lead `Run`,
//! else `Run` while any sub-agent runs (so `Stop` or a quiet lead does not end the session card
//! while children work), else the lead's own state.
//!
//! | Event | Context | Effect |
//! |---|---|---|
//! | `SessionStart` | `source` = `startup` / `resume` / `clear` | lead `Idle`, sub-agents cleared |
//! | `SessionStart` | `source` = `compact` | no change (compaction can happen mid-turn) |
//! | `UserPromptSubmit` | | lead `Run`; finished sub-agents of the previous turn dropped |
//! | `PreToolUse` | lead | lead `Run` (a denied permission never fires `PostToolUse`, so this clears `Perm`) |
//! | `PostToolUse` | lead | lead `Run`; clears a pending `Perm` of the same tool |
//! | `PostToolUseFailure` | lead | lead `Run`: the error goes back to the model and the turn continues; only `StopFailure` is `Fail` |
//! | `PermissionRequest` | lead | lead `Perm` with `PermRequest { tool, summary }` |
//! | `Stop` | | lead `Done` (reported `Run` while sub-agents still run) |
//! | `StopFailure` | lead | lead `Fail` |
//! | `SubagentStart` | | add / restart sub-agent `Run` (name = `agent_type`, else `agent_id`) |
//! | `PreToolUse`, `PostToolUse`, `PostToolUseFailure` | sub-agent (`agent_id` set) | that sub-agent `Run`, its `Perm` cleared |
//! | `PermissionRequest` | sub-agent | that sub-agent `Perm`; session reported `Perm` with the request |
//! | `StopFailure` | sub-agent | that sub-agent `Fail` |
//! | `SubagentStop` | | that sub-agent `Done`, or still `Fail` if it failed (kept in the list until the next `UserPromptSubmit`) |
//! | `UserPromptSubmit` | sub-agent silent > 10 min | that sub-agent dropped (its stop event was lost) |
//! | `Observation::Tick` | running sub-agent silent > 30 min | that sub-agent `Done` (its stop event was lost) |
//! | `SessionEnd` | | lead `Idle`, sub-agents cleared |
//! | PTY child exited | `Observation::Process` | like `SessionEnd`, if the adapter had seen any hook |
//!
//! Alpha does not answer permission decisions: `lotse-hook` never writes a decision to stdout, so
//! Claude's own prompt in the terminal stays the only place to Allow or Deny.

use crate::adapter::{Adapter, Detection, HookObservation, Observation};
use lotse_proto::{
    AgentKind, DetectSource, PermRequest, SessionId, Status, SubAgent, SubAgentId, UnixMs,
};
use serde_json::Value;

/// Hooks reference (see the module docs; **Unverified** against the live page).
pub const HOOKS_DOC_URL: &str = "https://code.claude.com/docs/en/hooks";

/// Longest `PermRequest::summary`, in characters.
const SUMMARY_MAX_CHARS: usize = 160;
/// Hook payloads come from any local process: every string kept in memory is clipped, and the
/// per-session sub-agent list is bounded (oldest dropped), so a flood cannot grow the daemon.
const TOOL_MAX_CHARS: usize = 40;
const ID_MAX_CHARS: usize = 128;
/// Text scanned for the one-line summary.
const COMMAND_MAX_CHARS: usize = 512;
const MAX_SUBS: usize = 256;

/// Hook delivery is best effort (`lotse-hook` drops an event on timeout or a daemon restart), so a
/// lost `SubagentStop` must not keep a session running forever. A running or waiting sub-agent that
/// has been silent this long is dropped when the user submits the next prompt...
const SUB_STALE_ON_PROMPT_MS: UnixMs = 10 * 60 * 1000;
/// ...and a running one is marked done by a clock tick after this long. Sub-agents emit tool events
/// while they work, so a silent half hour means they are gone.
const SUB_EXPIRE_ON_TICK_MS: UnixMs = 30 * 60 * 1000;

/// Called by `Engine::with_default_adapters`: the hook adapter for `AgentKind::ClaudeCode`.
pub fn default_adapter(_session: &SessionId, agent: AgentKind) -> Option<Box<dyn Adapter>> {
    (agent == AgentKind::ClaudeCode).then(|| Box::new(ClaudeAdapter::new()) as Box<dyn Adapter>)
}

/// A permission prompt that is waiting for the user.
#[derive(Debug, Clone, PartialEq)]
struct Pending {
    req: PermRequest,
    tool_use_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
enum Lead {
    Unknown,
    Idle,
    Run,
    Perm(Pending),
    Fail,
    Done,
}

#[derive(Debug, Clone, PartialEq)]
struct Sub {
    id: String,
    name: String,
    status: Status,
    /// Set while `status == Perm`.
    perm: Option<Pending>,
    /// Time of the last hook event about this sub-agent (self-heal when a stop event is lost).
    last_seen: UnixMs,
}

/// What the adapter last told the engine.
#[derive(Debug, Clone, PartialEq)]
struct Reported {
    status: Status,
    perm: Option<PermRequest>,
    sub_agents: Vec<SubAgent>,
}

/// Hook adapter for one Claude Code session. Codex's hooks have the same shape, so [`crate::codex`]
/// reuses it with a few differences (`ClaudeAdapter::for_agent(AgentKind::Codex)`).
#[derive(Debug)]
pub struct ClaudeAdapter {
    agent: AgentKind,
    lead: Lead,
    subs: Vec<Sub>,
    reported: Option<Reported>,
    /// Time of the observation being handled.
    now: UnixMs,
}

impl Default for ClaudeAdapter {
    fn default() -> Self {
        Self::new()
    }
}

impl ClaudeAdapter {
    pub fn new() -> Self {
        Self::for_agent(AgentKind::ClaudeCode)
    }

    /// The adapter for `agent`'s hooks: `ClaudeCode` or `Codex` (the differences are in
    /// [`crate::codex`]).
    pub(crate) fn for_agent(agent: AgentKind) -> Self {
        Self {
            agent,
            lead: Lead::Unknown,
            subs: Vec::new(),
            reported: None,
            now: 0,
        }
    }

    /// Apply one hook payload. `false`: not an event this adapter acts on.
    fn handle_hook(&mut self, payload: &Value, at: UnixMs) -> bool {
        self.now = at;
        let Some(event) = str_field(payload, "hook_event_name") else {
            return false;
        };
        let agent_id = bounded_field(payload, "agent_id", ID_MAX_CHARS);
        let codex = self.agent == AgentKind::Codex;
        if codex {
            // The status table of `codex.rs`: a sub-agent's own thread runs the lifecycle hooks too
            // (tagged with its `agent_id`), and a question for the user is a `PreToolUse`.
            if let Some(id) = agent_id
                && matches!(
                    event,
                    "SessionStart" | "UserPromptSubmit" | "Stop" | "Interrupt"
                )
            {
                let sub = self.upsert_sub(id, bounded_field(payload, "agent_type", TOOL_MAX_CHARS));
                sub.status = Status::Run;
                sub.perm = None;
                return true;
            }
            if event == "PreToolUse"
                && str_field(payload, "tool_name").is_some_and(crate::codex::is_user_input_tool)
            {
                return self.handle_hook_event("PermissionRequest", payload, agent_id);
            }
        }
        self.handle_hook_event(event, payload, agent_id)
    }

    /// `handle_hook` after the Codex rewrites, for `event` (`false`: not an event this adapter acts on).
    fn handle_hook_event(&mut self, event: &str, payload: &Value, agent_id: Option<&str>) -> bool {
        let at = self.now;
        match event {
            "SessionStart" => {
                if str_field(payload, "source") == Some("compact") {
                    return false;
                }
                // Codex runs it when a turn begins; Claude Code when the session does.
                self.lead = if self.agent == AgentKind::Codex {
                    Lead::Run
                } else {
                    Lead::Idle
                };
                self.subs.clear();
            }
            "UserPromptSubmit" => {
                self.lead = Lead::Run;
                self.subs.retain(|s| {
                    matches!(s.status, Status::Run | Status::Perm)
                        && at.saturating_sub(s.last_seen) <= SUB_STALE_ON_PROMPT_MS
                });
            }
            "PreToolUse" | "PostToolUse" | "PostToolUseFailure" => {
                let post = event != "PreToolUse";
                match agent_id {
                    Some(id) => {
                        let sub = self
                            .upsert_sub(id, bounded_field(payload, "agent_type", TOOL_MAX_CHARS));
                        if !post || perm_cleared_by(sub.perm.as_ref(), payload) {
                            sub.status = Status::Run;
                            sub.perm = None;
                        }
                    }
                    None => match &self.lead {
                        Lead::Perm(pending) if post && !perm_cleared_by(Some(pending), payload) => {
                        }
                        _ => self.lead = Lead::Run,
                    },
                }
            }
            "PermissionRequest" => {
                let pending = Pending {
                    req: perm_request(payload),
                    tool_use_id: bounded_field(payload, "tool_use_id", ID_MAX_CHARS)
                        .map(str::to_owned),
                };
                match agent_id {
                    Some(id) => {
                        let sub = self
                            .upsert_sub(id, bounded_field(payload, "agent_type", TOOL_MAX_CHARS));
                        sub.status = Status::Perm;
                        sub.perm = Some(pending);
                    }
                    None => self.lead = Lead::Perm(pending),
                }
            }
            "Stop" => self.lead = Lead::Done,
            // Esc in Codex: the turn ends without a Stop
            "Interrupt" if self.agent == AgentKind::Codex => self.lead = Lead::Done,
            "StopFailure" => match agent_id {
                Some(id) => {
                    let sub =
                        self.upsert_sub(id, bounded_field(payload, "agent_type", TOOL_MAX_CHARS));
                    sub.status = Status::Fail;
                    sub.perm = None;
                }
                None => self.lead = Lead::Fail,
            },
            "SubagentStart" => {
                let Some(id) = agent_id else { return false };
                let sub = self.upsert_sub(id, bounded_field(payload, "agent_type", TOOL_MAX_CHARS));
                sub.status = Status::Run;
                sub.perm = None;
            }
            "SubagentStop" => {
                let Some(id) = agent_id else { return false };
                // A stop for a child we never saw start (daemon restarted mid-session) adds no row.
                let Some(sub) = self.subs.iter_mut().find(|s| s.id == id) else {
                    return false;
                };
                // A child that failed stays failed.
                if sub.status != Status::Fail {
                    sub.status = Status::Done;
                }
                sub.perm = None;
            }
            "SessionEnd" => self.reset(),
            _ => return false,
        }
        true
    }

    fn reset(&mut self) {
        self.lead = Lead::Idle;
        self.subs.clear();
    }

    /// The row for `id`, created as `Run` when the start event was missed.
    fn upsert_sub(&mut self, id: &str, agent_type: Option<&str>) -> &mut Sub {
        let at = match self.subs.iter().position(|s| s.id == id) {
            Some(at) => at,
            None => {
                if self.subs.len() >= MAX_SUBS {
                    self.subs.remove(0);
                }
                self.subs.push(Sub {
                    id: id.to_owned(),
                    name: agent_type.unwrap_or(id).to_owned(),
                    status: Status::Run,
                    perm: None,
                    last_seen: self.now,
                });
                self.subs.len() - 1
            }
        };
        let sub = &mut self.subs[at];
        sub.last_seen = self.now;
        // The first event of a row may be a tool event without `agent_type`; take the name when it shows up.
        if let Some(agent_type) = agent_type.filter(|t| sub.name == sub.id && *t != sub.id) {
            sub.name = agent_type.to_owned();
        }
        sub
    }

    /// Derived status and permission request (see the status table).
    fn effective(&self) -> (Status, Option<PermRequest>) {
        if let Lead::Perm(pending) = &self.lead {
            return (Status::Perm, Some(pending.req.clone()));
        }
        if let Some(pending) = self.subs.iter().find_map(|s| s.perm.as_ref()) {
            return (Status::Perm, Some(pending.req.clone()));
        }
        let any_sub_running = self.subs.iter().any(|s| s.status == Status::Run);
        let status = match &self.lead {
            Lead::Fail => Status::Fail,
            Lead::Run => Status::Run,
            _ if any_sub_running => Status::Run,
            Lead::Idle => Status::Idle,
            Lead::Done => Status::Done,
            Lead::Unknown => Status::Unknown,
            Lead::Perm(_) => unreachable!("handled above"),
        };
        (status, None)
    }

    /// Compare with what was last reported; `Some` detection when something changed.
    fn detect(&mut self, at: u64) -> Vec<Detection> {
        let (status, perm) = self.effective();
        let sub_agents: Vec<SubAgent> = self
            .subs
            .iter()
            .map(|s| SubAgent {
                id: SubAgentId::new(s.id.as_str()),
                name: s.name.clone(),
                status: s.status,
            })
            .collect();
        let previous = self.reported.as_ref();
        let subs_changed = previous.is_none_or(|r| r.sub_agents != sub_agents);
        let head_changed = previous.is_none_or(|r| r.status != status || r.perm != perm);
        if !subs_changed && !head_changed {
            return Vec::new();
        }
        let detection = Detection {
            source: DetectSource::Hook,
            status: Some(status),
            sub_agents: subs_changed.then(|| sub_agents.clone()),
            perm: perm.clone(),
            at,
        };
        self.reported = Some(Reported {
            status,
            perm,
            sub_agents,
        });
        vec![detection]
    }
}

impl Adapter for ClaudeAdapter {
    fn name(&self) -> &'static str {
        match self.agent {
            AgentKind::Codex => "codex-hooks",
            _ => "claude-hooks",
        }
    }

    fn agent(&self) -> AgentKind {
        self.agent
    }

    fn source(&self) -> DetectSource {
        DetectSource::Hook
    }

    fn observe(&mut self, obs: &Observation) -> Vec<Detection> {
        match obs {
            Observation::Hook(HookObservation { agent, payload, at }) if *agent == self.agent => {
                if self.handle_hook(payload, *at) {
                    self.detect(*at)
                } else {
                    Vec::new()
                }
            }
            // Self-heal: a running sub-agent that went silent lost its stop event.
            Observation::Tick { now } => {
                let mut expired = false;
                for sub in &mut self.subs {
                    if sub.status == Status::Run
                        && now.saturating_sub(sub.last_seen) > SUB_EXPIRE_ON_TICK_MS
                    {
                        sub.status = Status::Done;
                        expired = true;
                    }
                }
                if expired {
                    self.detect(*now)
                } else {
                    Vec::new()
                }
            }
            // The PTY child is gone: a hook-driven `Run` would otherwise stay forever.
            Observation::Process(p)
                if p.foreground.is_none()
                    && (self.lead != Lead::Unknown || !self.subs.is_empty()) =>
            {
                self.reset();
                self.detect(p.at)
            }
            _ => Vec::new(),
        }
    }
}

/// Non-empty string field of a JSON object.
fn str_field<'a>(payload: &'a Value, key: &str) -> Option<&'a str> {
    payload.get(key)?.as_str().filter(|s| !s.is_empty())
}

/// [`str_field`] cut to `max_chars` characters (on a character boundary).
fn bounded_field<'a>(payload: &'a Value, key: &str, max_chars: usize) -> Option<&'a str> {
    str_field(payload, key).map(|s| clip_str(s, max_chars))
}

fn clip_str(s: &str, max_chars: usize) -> &str {
    s.char_indices()
        .nth(max_chars)
        .map_or(s, |(at, _)| &s[..at])
}

/// Does a `PostToolUse` / `PostToolUseFailure` finish the pending permission prompt?
///
/// Parallel tool calls can finish while another tool still waits for the user: the post event only
/// clears the prompt when it is about the same tool call (`tool_use_id`) or, without ids, the same tool.
fn perm_cleared_by(pending: Option<&Pending>, payload: &Value) -> bool {
    let Some(pending) = pending else { return true };
    if let (Some(waiting), Some(done)) = (
        &pending.tool_use_id,
        bounded_field(payload, "tool_use_id", ID_MAX_CHARS),
    ) {
        return waiting == done;
    }
    match bounded_field(payload, "tool_name", TOOL_MAX_CHARS) {
        Some(tool) => tool == pending.req.tool,
        None => true,
    }
}

/// `PermRequest` from a `PermissionRequest` payload: the tool name and a one-line summary of its input.
fn perm_request(payload: &Value) -> PermRequest {
    let tool = bounded_field(payload, "tool_name", TOOL_MAX_CHARS).unwrap_or("Tool");
    let input = payload.get("tool_input");
    PermRequest {
        tool: tool.to_owned(),
        summary: input.map(|i| summarize_input(tool, i)).unwrap_or_default(),
    }
}

/// The `tool_input` key that best describes a call, per tool (field names of Claude Code's built-in
/// tools; **Unverified** against the live tools reference). Unknown tools fall back to the first
/// key of `COMMON_KEYS` that is present.
fn summarize_input(tool: &str, input: &Value) -> String {
    const COMMON_KEYS: [&str; 7] = [
        "command",
        "file_path",
        "path",
        "url",
        "pattern",
        "query",
        "description",
    ];
    let preferred: &[&str] = match tool {
        "Bash" => &["command"],
        "Edit" | "MultiEdit" | "Write" | "Read" => &["file_path"],
        "NotebookEdit" => &["notebook_path"],
        "WebFetch" => &["url"],
        "WebSearch" => &["query"],
        "Glob" | "Grep" => &["pattern"],
        _ => &[],
    };
    preferred
        .iter()
        .chain(COMMON_KEYS.iter())
        .find_map(|key| str_field(input, key))
        .map(|text| one_line(clip_str(text, COMMAND_MAX_CHARS)))
        .unwrap_or_default()
}

/// First non-empty line, trimmed and cut to [`SUMMARY_MAX_CHARS`] characters.
fn one_line(text: &str) -> String {
    let line = text
        .lines()
        .map(str::trim)
        .find(|l| !l.is_empty())
        .unwrap_or("");
    if line.chars().count() <= SUMMARY_MAX_CHARS {
        return line.to_owned();
    }
    let mut cut: String = line.chars().take(SUMMARY_MAX_CHARS - 1).collect();
    cut.push('…');
    cut
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::adapter::ProcessObservation;
    use serde_json::json;

    fn hook(adapter: &mut ClaudeAdapter, payload: Value, at: u64) -> Vec<Detection> {
        adapter.observe(&Observation::Hook(HookObservation {
            agent: AgentKind::ClaudeCode,
            payload,
            at,
        }))
    }

    #[test]
    fn summary_picks_the_tool_specific_field() {
        let p = perm_request(&json!({
            "tool_name": "Bash",
            "tool_input": {"command": "npx playwright test e2e/patients", "description": "Run e2e"}
        }));
        assert_eq!(p.tool, "Bash");
        assert_eq!(p.summary, "npx playwright test e2e/patients");
        let p = perm_request(
            &json!({"tool_name": "Write", "tool_input": {"file_path": "/a/b.ts", "content": "x"}}),
        );
        assert_eq!(p.summary, "/a/b.ts");
        // Unknown tool: first common key; MCP tool with no known key: empty summary.
        let p = perm_request(
            &json!({"tool_name": "mcp__x__y", "tool_input": {"url": "https://e.com"}}),
        );
        assert_eq!(p.summary, "https://e.com");
        let p = perm_request(&json!({"tool_name": "mcp__x__y", "tool_input": {"foo": 1}}));
        assert_eq!(p.summary, "");
        // No tool name at all.
        assert_eq!(perm_request(&json!({})).tool, "Tool");
    }

    #[test]
    fn hook_strings_are_clipped_on_a_char_boundary() {
        let p = perm_request(&json!({
            "tool_name": "ü".repeat(10_000),
            "tool_input": {"command": format!("{}x", " ".repeat(100_000))}
        }));
        assert_eq!(p.tool.chars().count(), TOOL_MAX_CHARS);
        assert!(p.summary.chars().count() <= SUMMARY_MAX_CHARS);
    }

    #[test]
    fn sub_agent_rows_are_capped_dropping_the_oldest() {
        let mut a = ClaudeAdapter::new();
        for n in 0..(MAX_SUBS * 2) {
            let id = format!("{n}-{}", "i".repeat(5_000));
            hook(
                &mut a,
                json!({"hook_event_name": "SubagentStart", "agent_id": id}),
                n as u64,
            );
        }
        assert_eq!(a.subs.len(), MAX_SUBS);
        assert!(a.subs.iter().all(|s| s.id.chars().count() <= ID_MAX_CHARS));
        assert!(a.subs[0].id.starts_with(&format!("{}-", MAX_SUBS)));
    }

    #[test]
    fn summary_is_one_short_line() {
        let long = format!("echo {}\nsecond line", "x".repeat(400));
        let s = one_line(&long);
        assert_eq!(s.chars().count(), SUMMARY_MAX_CHARS);
        assert!(s.ends_with('…'));
        assert_eq!(one_line("\n  ls -la  \nnext"), "ls -la");
        // Multi-byte characters are cut on a character boundary.
        let s = one_line(&"ü".repeat(300));
        assert_eq!(s.chars().count(), SUMMARY_MAX_CHARS);
    }

    #[test]
    fn foreign_agent_and_unknown_events_are_ignored() {
        let mut a = ClaudeAdapter::new();
        let foreign = Observation::Hook(HookObservation {
            agent: AgentKind::Codex,
            payload: json!({"hook_event_name": "UserPromptSubmit"}),
            at: 1,
        });
        assert!(a.observe(&foreign).is_empty());
        assert!(hook(&mut a, json!({"hook_event_name": "Notification"}), 2).is_empty());
        assert!(hook(&mut a, json!({"hook_event_name": "PreCompact"}), 3).is_empty());
        assert!(hook(&mut a, json!({"no_event": true}), 4).is_empty());
        assert!(hook(&mut a, json!("not an object"), 5).is_empty());
        assert!(a.observe(&Observation::Tick { now: 6 }).is_empty());
    }

    #[test]
    fn repeated_events_report_only_changes() {
        let mut a = ClaudeAdapter::new();
        assert_eq!(
            hook(&mut a, json!({"hook_event_name": "UserPromptSubmit"}), 1).len(),
            1
        );
        assert!(
            hook(
                &mut a,
                json!({"hook_event_name": "PreToolUse", "tool_name": "Bash"}),
                2
            )
            .is_empty()
        );
        assert!(
            hook(
                &mut a,
                json!({"hook_event_name": "PostToolUse", "tool_name": "Bash"}),
                3
            )
            .is_empty()
        );
    }

    #[test]
    fn process_exit_resets_only_after_a_hook_was_seen() {
        let exit = |at| {
            Observation::Process(ProcessObservation {
                foreground: None,
                exit_code: Some(0),
                at,
            })
        };
        let mut a = ClaudeAdapter::new();
        assert!(
            a.observe(&exit(1)).is_empty(),
            "no hook signal yet: no opinion"
        );
        hook(&mut a, json!({"hook_event_name": "UserPromptSubmit"}), 2);
        let d = a.observe(&exit(3));
        assert_eq!(d.len(), 1);
        assert_eq!(d[0].status, Some(Status::Idle));
        // A still-running foreground process is not an exit.
        hook(&mut a, json!({"hook_event_name": "UserPromptSubmit"}), 4);
        let alive = Observation::Process(ProcessObservation {
            foreground: Some("claude".into()),
            exit_code: None,
            at: 5,
        });
        assert!(a.observe(&alive).is_empty());
    }

    #[test]
    fn a_lost_subagent_stop_heals_on_the_next_prompt_or_a_late_tick() {
        const MIN: u64 = 60_000;
        let start =
            json!({"hook_event_name": "SubagentStart", "agent_id": "ag-1", "agent_type": "w"});
        let prompt = json!({"hook_event_name": "UserPromptSubmit"});
        let stop = json!({"hook_event_name": "Stop"});

        // Next prompt after 11 silent minutes drops the orphan; after 2 minutes it stays.
        for (gap, kept) in [(2 * MIN, 1), (11 * MIN, 0)] {
            let mut a = ClaudeAdapter::new();
            hook(&mut a, prompt.clone(), 0);
            hook(&mut a, start.clone(), 1000);
            hook(&mut a, stop.clone(), 2000);
            let d = hook(&mut a, prompt.clone(), 1000 + gap);
            assert_eq!(a.subs.len(), kept, "gap {gap}");
            assert_eq!(a.effective().0, Status::Run);
            // Kept: nothing visible changes (still running). Dropped: the list is reported empty.
            assert_eq!(d.is_empty(), kept == 1, "gap {gap}");
            if kept == 0 {
                assert_eq!(d[0].sub_agents, Some(vec![]));
            }
        }

        // A tick marks a long-silent running sub-agent done, so the session settles to Done.
        let mut a = ClaudeAdapter::new();
        hook(&mut a, prompt.clone(), 0);
        hook(&mut a, start, 1000);
        hook(&mut a, stop, 2000);
        assert_eq!(a.effective().0, Status::Run);
        assert!(
            a.observe(&Observation::Tick {
                now: 1000 + 29 * MIN
            })
            .is_empty()
        );
        let d = a.observe(&Observation::Tick {
            now: 1000 + 31 * MIN,
        });
        assert_eq!(d[0].status, Some(Status::Done));
        assert_eq!(d[0].sub_agents.as_ref().unwrap()[0].status, Status::Done);
    }

    #[test]
    fn default_adapter_is_claude_only() {
        let id = SessionId::new("a1");
        assert!(default_adapter(&id, AgentKind::ClaudeCode).is_some());
        for other in [AgentKind::Terminal, AgentKind::Codex, AgentKind::OpenCode] {
            assert!(default_adapter(&id, other).is_none());
        }
    }
}
