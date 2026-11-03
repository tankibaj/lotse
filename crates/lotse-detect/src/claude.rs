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
//! | `Stop` | lead | lead `Done` (reported `Run` while sub-agents still run, or while the event's `background_tasks` is non-empty) |
//! | `Stop` | sub-agent (`agent_id` set) | that sub-agent `Done` (or still `Fail`) |
//! | `StopFailure` | lead | lead `Fail` |
//! | `SubagentStart` | | add / restart sub-agent `Run` (name = `agent_type`, else `agent_id`); adopts the row an Agent tool call made |
//! | `PreToolUse` | tool `Agent` / `Task` | add sub-agent `Run` keyed by `tool_use_id` (name = `description`, else `subagent_type`); `run_in_background` is remembered |
//! | `PostToolUse` | tool `Agent` / `Task`, foreground | that sub-agent `Done` (`Fail` when the response says it failed) |
//! | `PostToolUse` | tool `Agent` / `Task`, `tool_response.status` = `async_launched` | the sub-agent keeps running (background); its `agentId` becomes an alias |
//! | `PostToolUseFailure` | tool `Agent` / `Task` | that sub-agent `Fail` |
//! | `PreToolUse`, `PostToolUse`, `PostToolUseFailure` | sub-agent (`agent_id` set) | that sub-agent `Run`, its `Perm` cleared |
//! | `PermissionRequest` | sub-agent | that sub-agent `Perm`; session reported `Perm` with the request |
//! | `Notification` | `notification_type` = `permission_prompt` | session `Perm` (placeholder request) unless a `PermissionRequest` already said so; ends with the next tool event of that tool, `Stop` or a prompt |
//! | `StopFailure` | sub-agent | that sub-agent `Fail` |
//! | `SubagentStop` | | that sub-agent `Done`, or still `Fail` if it failed (kept in the list until the next `UserPromptSubmit`) |
//! | `UserPromptSubmit` | the prompt holds `<task-notification>` blocks | lead `Run`; each block closes its sub-agent (`Done`, `Fail` for a failed status or an `API Error` result); nothing is dropped |
//! | `TaskNotification` | synthetic, from the daemon reading the transcript | like the block above, without touching the lead |
//! | `UserPromptSubmit` | sub-agent silent > 10 min (background ones: > 2 h) | that sub-agent dropped (its stop event was lost) |
//! | `Observation::Tick` | running sub-agent silent > 30 min (background: > 2 h) | that sub-agent `Done` (its stop event was lost) |
//! | `SessionEnd` | | lead `Idle`, sub-agents cleared |
//! | PTY child exited | `Observation::Process` | like `SessionEnd`, if the adapter had seen any hook |
//!
//! # What Claude Code really sends for background agents (live walk, v2.1.311)
//!
//! "Launch 5 general-purpose subagents in parallel" ran five *background* agents: `PreToolUse` and
//! `PostToolUse` (tool `Agent`) for each, the lead's `Stop` right away, no `SubagentStart`,
//! `SubagentStop` or `PermissionRequest`, and each agent reported back later as a new user turn whose
//! text is a `<task-notification>` block (`task-id`, `tool-use-id`, `status`, `summary`, `result`,
//! `usage`). The adapter therefore tracks sub-agents from the Agent tool's own events and closes a
//! background one from its notification: the `UserPromptSubmit` of the report-back turn when Claude
//! fires one, and always from the transcript, which the daemon reads ([`parse_task_notifications`]).
//! `Task` is the tool's older name.
//!
//! A failed background agent is told from its notification: `status` other than `completed` /
//! `killed` / `stopped` / `cancelled`, or a result that starts with `API Error`. An agent the user
//! stopped is `Done`, not `Fail` (nothing broke).
//!
//! **Unverified** (no recording; the shapes follow the live walk and the hooks reference, the
//! fixtures are authored): the `tool_response` fields of an async launch (`status`, `agentId`), the
//! shape of `Stop`'s `background_tasks` (any non-empty array, object or positive number counts).
//!
//! Alpha does not answer permission decisions: `lotse-hook` never writes a decision to stdout, so
//! Claude's own prompt in the terminal stays the only place to Allow or Deny. A prompt Claude draws
//! but no hook reports (a background agent's) is still caught from the screen: see
//! `Engine`'s merge rule.

use crate::adapter::{Adapter, Detection, HookObservation, Observation};
use lotse_proto::{
    AgentKind, DetectSource, PermRequest, SessionId, Status, SubAgent, SubAgentId, UnixMs,
};
use serde_json::{Value, json};

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
/// A background agent (or a `Stop` that reported background tasks) can be silent for a long time:
/// its report-back is the signal, so only a lost one is given up on, after this long.
const BG_EXPIRE_MS: UnixMs = 2 * 60 * 60 * 1000;
/// Names of the tool that starts a sub-agent (`Task` is the older name).
const AGENT_TOOLS: [&str; 2] = ["Agent", "Task"];
/// Longest sub-agent name (the Agent tool's `description`), in characters.
const NAME_MAX_CHARS: usize = 60;
/// `<task-notification>` blocks read from one text.
const MAX_NOTIFICATIONS: usize = 64;
/// What a permission request says when only the fact that Claude asks is known (a `Notification`).
/// The screen adapter uses the same words.
pub const PLACEHOLDER_PERM_SUMMARY: &str = "Waiting for your input";

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

/// How a hook's tool event relates to a [`Pending`] prompt.
fn tool_name_matches(pending: &Pending, tool: Option<&str>) -> bool {
    tool.is_none_or(|t| pending.req.tool == PLACEHOLDER_PERM_TOOL || t == pending.req.tool)
}

/// The tool of a prompt only a `Notification` reported (its message names none we can read).
const PLACEHOLDER_PERM_TOOL: &str = "approval";

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
    /// Other ids this agent goes by: a hook's `agent_id`, the `agentId` of an async launch.
    aliases: Vec<String>,
    /// `subagent_type` of the Agent tool call that made the row.
    agent_type: Option<String>,
    /// Made by an Agent tool call (its id is the `tool_use_id`).
    tool: bool,
    /// A hook `agent_id` was matched with this row (a `tool` row waits for one until then).
    linked: bool,
    /// Runs in the background: it reports back as a task notification, possibly hours later.
    background: bool,
}

impl Sub {
    fn is(&self, id: &str) -> bool {
        self.id == id || self.aliases.iter().any(|a| a == id)
    }
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
    /// A prompt only a `Notification` reported; any `PermissionRequest` replaces it.
    weak_perm: Option<Pending>,
    /// When the last `Stop` reported background tasks (it still counts as running until a `Stop`
    /// without any, or [`BG_EXPIRE_MS`]).
    bg_pending: Option<UnixMs>,
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
            weak_perm: None,
            bg_pending: None,
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
        let claude = self.agent == AgentKind::ClaudeCode;
        match event {
            // a sub-agent's own session events are not the session's
            "SessionStart" | "SessionEnd" | "UserPromptSubmit" if claude && agent_id.is_some() => {
                return false;
            }
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
                self.weak_perm = None;
                self.bg_pending = None;
            }
            "UserPromptSubmit" => {
                self.lead = Lead::Run;
                self.weak_perm = None;
                // The report-back turn of background agents: it closes them, and drops nothing.
                let notes = claude
                    .then(|| str_field(payload, "prompt").map(parse_task_notifications))
                    .flatten()
                    .unwrap_or_default();
                if notes.is_empty() {
                    self.subs.retain(|s| {
                        let quiet = at.saturating_sub(s.last_seen);
                        matches!(s.status, Status::Run | Status::Perm)
                            && quiet
                                <= if s.background {
                                    BG_EXPIRE_MS
                                } else {
                                    SUB_STALE_ON_PROMPT_MS
                                }
                    });
                } else {
                    for note in &notes {
                        self.close_by_notification(note);
                    }
                }
            }
            "TaskNotification" if claude => {
                let note = task_notification_from_event(payload);
                if !self.close_by_notification(&note) {
                    return false;
                }
            }
            "PreToolUse" | "PostToolUse" | "PostToolUseFailure" => {
                let post = event != "PreToolUse";
                let tool = bounded_field(payload, "tool_name", TOOL_MAX_CHARS);
                // the tool went ahead (or was refused): a prompt only a notification knew of is over
                if self
                    .weak_perm
                    .as_ref()
                    .is_some_and(|w| tool_name_matches(w, tool))
                {
                    self.weak_perm = None;
                }
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
                if claude && tool.is_some_and(|t| AGENT_TOOLS.contains(&t)) {
                    self.track_agent_tool(event, payload);
                }
            }
            "PermissionRequest" => {
                self.weak_perm = None;
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
            // "Claude needs your permission to use Bash": the prompt is open, the request unknown
            "Notification" if claude => {
                if str_field(payload, "notification_type") != Some("permission_prompt")
                    || matches!(self.lead, Lead::Perm(_))
                    || self.subs.iter().any(|s| s.perm.is_some())
                {
                    return false;
                }
                self.weak_perm = Some(Pending {
                    req: PermRequest {
                        tool: notification_tool(str_field(payload, "message")),
                        summary: PLACEHOLDER_PERM_SUMMARY.to_owned(),
                        always: None,
                        id: None,
                    },
                    tool_use_id: None,
                });
            }
            "Stop" => match agent_id {
                // a sub-agent's own stop
                Some(id) if claude => {
                    let Some(sub) = self.subs.iter_mut().find(|s| s.is(id)) else {
                        return false;
                    };
                    if sub.status != Status::Fail {
                        sub.status = Status::Done;
                    }
                    sub.perm = None;
                }
                _ => {
                    self.lead = Lead::Done;
                    self.weak_perm = None;
                    self.bg_pending = (claude && has_background_tasks(payload)).then_some(at);
                }
            },
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
                let Some(sub) = self.subs.iter_mut().find(|s| s.is(id)) else {
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
        self.weak_perm = None;
        self.bg_pending = None;
    }

    /// The row for `id`, created as `Run` when the start event was missed. An id no row knows is
    /// first offered to a row an Agent tool call made and no agent id was matched with yet (the
    /// tool event comes before the agent's own); only then is a new row made.
    fn upsert_sub(&mut self, id: &str, agent_type: Option<&str>) -> &mut Sub {
        let at = match self.subs.iter().position(|s| s.is(id)) {
            Some(at) => at,
            None => match self.unlinked_tool_row(agent_type) {
                Some(at) => {
                    let sub = &mut self.subs[at];
                    sub.aliases.push(id.to_owned());
                    sub.linked = true;
                    at
                }
                None => {
                    self.push_sub(Sub {
                        id: id.to_owned(),
                        name: agent_type.unwrap_or(id).to_owned(),
                        status: Status::Run,
                        perm: None,
                        last_seen: self.now,
                        aliases: Vec::new(),
                        agent_type: None,
                        tool: false,
                        linked: true,
                        background: false,
                    });
                    self.subs.len() - 1
                }
            },
        };
        let sub = &mut self.subs[at];
        sub.last_seen = self.now;
        // The first event of a row may be a tool event without `agent_type`; take the name when it shows up.
        if let Some(agent_type) = agent_type.filter(|t| sub.name == sub.id && *t != sub.id) {
            sub.name = agent_type.to_owned();
        }
        sub
    }

    fn push_sub(&mut self, sub: Sub) {
        if self.subs.len() >= MAX_SUBS {
            self.subs.remove(0);
        }
        self.subs.push(sub);
    }

    /// The oldest running Agent-tool row no agent id was matched with, preferring one of `agent_type`
    /// (a call without `subagent_type` is a `general-purpose` agent).
    fn unlinked_tool_row(&self, agent_type: Option<&str>) -> Option<usize> {
        let open = |s: &Sub| s.tool && !s.linked && s.status == Status::Run;
        self.subs
            .iter()
            .position(|s| {
                open(s)
                    && agent_type.is_some_and(|t| s.agent_type.as_deref().unwrap_or(GENERAL) == t)
            })
            .or_else(|| self.subs.iter().position(open))
    }

    /// An Agent (or Task) tool event of the lead or of another sub-agent: it makes, keeps or ends the
    /// row of the agent it starts. `event` is the hook event name.
    fn track_agent_tool(&mut self, event: &str, payload: &Value) {
        let Some(id) = bounded_field(payload, "tool_use_id", ID_MAX_CHARS) else {
            return;
        };
        let input = payload.get("tool_input");
        let response = payload.get("tool_response");
        let launched_async = response.is_some_and(is_async_launch);
        let known = self.subs.iter().position(|s| s.id == id);
        match (event, known) {
            ("PreToolUse", Some(at)) => self.subs[at].last_seen = self.now,
            ("PreToolUse", None) => {
                let agent_type = input
                    .and_then(|i| bounded_field(i, "subagent_type", TOOL_MAX_CHARS))
                    .map(str::to_owned);
                let name = input
                    .and_then(|i| bounded_field(i, "description", NAME_MAX_CHARS))
                    .or(agent_type.as_deref())
                    .unwrap_or("agent")
                    .to_owned();
                self.push_sub(Sub {
                    id: id.to_owned(),
                    name,
                    status: Status::Run,
                    perm: None,
                    last_seen: self.now,
                    aliases: Vec::new(),
                    agent_type,
                    tool: true,
                    linked: false,
                    background: input
                        .and_then(|i| i.get("run_in_background"))
                        .and_then(Value::as_bool)
                        .unwrap_or(false),
                });
            }
            // the launch is seen but not its start (the daemon came in late): a background launch
            // still runs, so it gets a row; a finished foreground call needs none
            ("PostToolUse", None) if launched_async => {
                self.push_sub(Sub {
                    id: id.to_owned(),
                    name: response
                        .and_then(|r| bounded_field(r, "description", NAME_MAX_CHARS))
                        .unwrap_or("agent")
                        .to_owned(),
                    status: Status::Run,
                    perm: None,
                    last_seen: self.now,
                    aliases: Vec::new(),
                    agent_type: None,
                    tool: true,
                    linked: false,
                    background: true,
                });
                let at = self.subs.len() - 1;
                self.note_async_launch(at, response);
            }
            ("PostToolUse", Some(at)) if launched_async => self.note_async_launch(at, response),
            ("PostToolUse", Some(at)) => {
                let sub = &mut self.subs[at];
                sub.last_seen = self.now;
                sub.status = if response.is_some_and(tool_response_failed) {
                    Status::Fail
                } else {
                    Status::Done
                };
                sub.perm = None;
            }
            ("PostToolUseFailure", Some(at)) => {
                let sub = &mut self.subs[at];
                sub.last_seen = self.now;
                sub.status = Status::Fail;
                sub.perm = None;
            }
            _ => {}
        }
    }

    /// The Agent tool answered with a background launch: the agent runs on, under its own id too.
    fn note_async_launch(&mut self, at: usize, response: Option<&Value>) {
        let ids: Vec<String> = response
            .into_iter()
            .flat_map(|r| ["agentId", "agent_id", "taskId", "task_id"].map(|k| r.get(k)))
            .flatten()
            .filter_map(Value::as_str)
            .filter(|s| !s.is_empty())
            .map(|s| clip_str(s, ID_MAX_CHARS).to_owned())
            .collect();
        let sub = &mut self.subs[at];
        sub.last_seen = self.now;
        sub.background = true;
        if sub.status != Status::Fail {
            sub.status = Status::Run;
        }
        for id in ids {
            if !sub.is(&id) {
                sub.aliases.push(id);
            }
        }
    }

    /// A task notification ends the sub-agent it names. `false`: no such row (it was never seen, or
    /// already gone), nothing changed.
    fn close_by_notification(&mut self, note: &TaskNotification) -> bool {
        let by_id = |s: &Sub| {
            [note.tool_use_id.as_deref(), note.task_id.as_deref()]
                .into_iter()
                .flatten()
                .any(|id| s.is(id))
        };
        let at = self.subs.iter().position(by_id).or_else(|| {
            // last resort: the only running row with the name the summary gives
            let name = summary_agent_name(&note.summary)?;
            let mut named = self
                .subs
                .iter()
                .enumerate()
                .filter(|(_, s)| s.name == name && s.status == Status::Run);
            match (named.next(), named.next()) {
                (Some((at, _)), None) => Some(at),
                _ => None,
            }
        });
        let Some(at) = at else { return false };
        let sub = &mut self.subs[at];
        sub.status = note.outcome();
        sub.perm = None;
        sub.last_seen = self.now;
        true
    }

    /// Derived status and permission request (see the status table).
    fn effective(&self) -> (Status, Option<PermRequest>) {
        if let Lead::Perm(pending) = &self.lead {
            return (Status::Perm, Some(pending.req.clone()));
        }
        if let Some(pending) = self.subs.iter().find_map(|s| s.perm.as_ref()) {
            return (Status::Perm, Some(pending.req.clone()));
        }
        if let Some(pending) = &self.weak_perm {
            return (Status::Perm, Some(pending.req.clone()));
        }
        let any_sub_running =
            self.subs.iter().any(|s| s.status == Status::Run) || self.bg_pending.is_some();
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
                    let limit = if sub.background {
                        BG_EXPIRE_MS
                    } else {
                        SUB_EXPIRE_ON_TICK_MS
                    };
                    if sub.status == Status::Run && now.saturating_sub(sub.last_seen) > limit {
                        sub.status = Status::Done;
                        expired = true;
                    }
                }
                if self
                    .bg_pending
                    .is_some_and(|since| now.saturating_sub(since) > BG_EXPIRE_MS)
                {
                    self.bg_pending = None;
                    expired = true;
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

/// `subagent_type` of an Agent call that names none.
const GENERAL: &str = "general-purpose";

/// Did the Agent tool answer with a background launch (the agent runs on)?
fn is_async_launch(response: &Value) -> bool {
    str_field(response, "status") == Some("async_launched")
        || response.get("isAsync").and_then(Value::as_bool) == Some(true)
}

/// Does the answer of a finished (foreground) Agent call say the agent failed?
fn tool_response_failed(response: &Value) -> bool {
    response.get("is_error").and_then(Value::as_bool) == Some(true)
        || str_field(response, "status")
            .is_some_and(|s| matches!(s.to_ascii_lowercase().as_str(), "failed" | "error"))
}

/// Does a `Stop` say background work (agents, shells) still runs? Any non-empty array or object, or
/// a positive number, under `background_tasks`.
fn has_background_tasks(payload: &Value) -> bool {
    match payload.get("background_tasks") {
        Some(Value::Array(a)) => !a.is_empty(),
        Some(Value::Object(o)) => !o.is_empty(),
        Some(Value::Number(n)) => n.as_u64().is_some_and(|n| n > 0),
        _ => false,
    }
}

/// The tool a permission `Notification` names: "Claude needs your permission to use Bash" → `Bash`.
fn notification_tool(message: Option<&str>) -> String {
    message
        .and_then(|m| m.split_once(" to use "))
        .and_then(|(_, tool)| tool.split_whitespace().next())
        .map(|t| clip_str(t.trim_end_matches(['.', ':', ',']), TOOL_MAX_CHARS))
        .filter(|t| !t.is_empty())
        .unwrap_or(PLACEHOLDER_PERM_TOOL)
        .to_owned()
}

// ---- task notifications -------------------------------------------------------------------------

/// One `<task-notification>` block: how Claude Code tells the lead that a background agent finished.
/// It arrives as the text of a user turn (a `UserPromptSubmit` prompt, a transcript line).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct TaskNotification {
    pub task_id: Option<String>,
    /// The `tool_use_id` of the Agent call that started the agent.
    pub tool_use_id: Option<String>,
    /// `completed`, `failed`, `killed`, ...
    pub status: String,
    /// `Agent "Count files" finished`
    pub summary: String,
    /// The agent's answer (cut to [`RESULT_MAX_CHARS`]).
    pub result: String,
}

/// Longest `result` kept of a notification, in characters.
pub const RESULT_MAX_CHARS: usize = 2_000;

impl TaskNotification {
    /// How the agent ended: `Fail` for a status that says it failed, or a result that is Claude
    /// Code's own `API Error` line; `Done` otherwise, including a user's stop (nothing broke).
    pub fn outcome(&self) -> Status {
        let failed = matches!(
            self.status.to_ascii_lowercase().as_str(),
            "failed" | "error" | "errored" | "timeout" | "timed_out"
        ) || self.result.trim_start().starts_with("API Error");
        if failed { Status::Fail } else { Status::Done }
    }
}

/// The `<task-notification>` blocks in `text` (at most [`MAX_NOTIFICATIONS`]).
pub fn parse_task_notifications(text: &str) -> Vec<TaskNotification> {
    const OPEN: &str = "<task-notification>";
    const CLOSE: &str = "</task-notification>";
    let mut out = Vec::new();
    let mut rest = text;
    while out.len() < MAX_NOTIFICATIONS
        && let Some(start) = rest.find(OPEN)
    {
        let body = &rest[start + OPEN.len()..];
        let end = body.find(CLOSE).unwrap_or(body.len());
        let block = &body[..end];
        out.push(TaskNotification {
            task_id: tag(block, "task-id", false).map(|s| clip_str(s, ID_MAX_CHARS).to_owned()),
            tool_use_id: tag(block, "tool-use-id", false)
                .map(|s| clip_str(s, ID_MAX_CHARS).to_owned()),
            status: tag(block, "status", false)
                .map(|s| clip_str(s, TOOL_MAX_CHARS).to_owned())
                .unwrap_or_default(),
            summary: tag(block, "summary", false)
                .map(|s| clip_str(s, COMMAND_MAX_CHARS).to_owned())
                .unwrap_or_default(),
            // the result may hold anything, tags included: up to the last closing tag
            result: tag(block, "result", true)
                .map(|s| clip_str(s, RESULT_MAX_CHARS).to_owned())
                .unwrap_or_default(),
        });
        rest = &body[end..];
    }
    out
}

/// `text` without its `<task-notification>` blocks (an unclosed one runs to the end), trimmed.
pub fn strip_task_notifications(text: &str) -> String {
    const OPEN: &str = "<task-notification>";
    const CLOSE: &str = "</task-notification>";
    let mut out = String::new();
    let mut rest = text;
    while let Some(start) = rest.find(OPEN) {
        out.push_str(&rest[..start]);
        let body = &rest[start + OPEN.len()..];
        rest = match body.find(CLOSE) {
            Some(end) => &body[end + CLOSE.len()..],
            None => "",
        };
    }
    out.push_str(rest);
    out.trim().to_owned()
}

/// The text between `<name>` and its closing tag (the last one with `last`), trimmed.
fn tag<'a>(block: &'a str, name: &str, last: bool) -> Option<&'a str> {
    let open = format!("<{name}>");
    let close = format!("</{name}>");
    let from = block.find(&open)? + open.len();
    let inner = &block[from..];
    let to = if last {
        inner.rfind(&close)
    } else {
        inner.find(&close)
    }
    .unwrap_or(inner.len());
    Some(inner[..to].trim())
}

/// The agent a summary names: `Agent "Count files" finished` → `Count files`.
pub fn summary_agent_name(summary: &str) -> Option<&str> {
    let (_, rest) = summary.split_once('"')?;
    let (name, _) = rest.rsplit_once('"')?;
    Some(name).filter(|n| !n.is_empty())
}

/// The synthetic `TaskNotification` event the daemon builds from a transcript line (see
/// [`task_notification_event`]) back into a notification.
fn task_notification_from_event(payload: &Value) -> TaskNotification {
    let text = |k: &str, max: usize| {
        str_field(payload, k)
            .map(|s| clip_str(s, max).to_owned())
            .unwrap_or_default()
    };
    TaskNotification {
        task_id: bounded_field(payload, "task_id", ID_MAX_CHARS).map(str::to_owned),
        tool_use_id: bounded_field(payload, "tool_use_id", ID_MAX_CHARS).map(str::to_owned),
        status: text("status", TOOL_MAX_CHARS),
        summary: text("summary", COMMAND_MAX_CHARS),
        result: text("result", RESULT_MAX_CHARS),
    }
}

/// The hook payload the daemon feeds the adapter for a notification it read from the transcript:
/// the hooks may not fire for the report-back turn, the transcript always has it.
pub fn task_notification_event(note: &TaskNotification) -> Value {
    json!({
        "hook_event_name": "TaskNotification",
        "task_id": note.task_id,
        "tool_use_id": note.tool_use_id,
        "status": note.status,
        "summary": note.summary,
        "result": note.result,
    })
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
        always: None,
        id: None,
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

    const NOTE: &str = "<task-notification>\n<task-id>a1</task-id>\n<tool-use-id>toolu_1</tool-use-id>\n<status>completed</status>\n<summary>Agent \"Count files\" finished</summary>\n<result>12 files, see <result>x</result></result>\n<usage>total_tokens: 9</usage>\n</task-notification>";

    #[test]
    fn task_notifications_are_read_from_a_user_turn_and_failures_are_told_apart() {
        let text = format!(
            "{NOTE}\n\nand\n{}",
            NOTE.replace("a1", "a2").replace("completed", "failed")
        );
        let notes = parse_task_notifications(&text);
        assert_eq!(notes.len(), 2);
        assert_eq!(notes[0].task_id.as_deref(), Some("a1"));
        assert_eq!(notes[0].tool_use_id.as_deref(), Some("toolu_1"));
        assert_eq!(summary_agent_name(&notes[0].summary), Some("Count files"));
        // the result runs to the last closing tag, tags inside it included
        assert_eq!(notes[0].result, "12 files, see <result>x</result>");
        assert_eq!(
            (notes[0].outcome(), notes[1].outcome()),
            (Status::Done, Status::Fail)
        );
        let api = TaskNotification {
            result: "API Error: 529 overloaded".into(),
            status: "completed".into(),
            ..TaskNotification::default()
        };
        assert_eq!(api.outcome(), Status::Fail);
        // a stop by the user is not a failure; neither is a plain answer about errors
        for (status, result) in [
            ("killed", ""),
            ("stopped", ""),
            ("completed", "Error handling reviewed"),
        ] {
            let n = TaskNotification {
                status: status.into(),
                result: result.into(),
                ..Default::default()
            };
            assert_eq!(n.outcome(), Status::Done, "{status} {result}");
        }
        // no block, an unclosed block, a flood
        assert!(parse_task_notifications("just a prompt").is_empty());
        assert_eq!(
            parse_task_notifications("<task-notification><status>failed").len(),
            1
        );
        assert_eq!(
            parse_task_notifications(&NOTE.repeat(200)).len(),
            MAX_NOTIFICATIONS
        );
    }

    #[test]
    fn helpers_read_the_odd_shapes_of_notifications_and_stops() {
        assert_eq!(
            notification_tool(Some("Claude needs your permission to use Bash")),
            "Bash"
        );
        assert_eq!(
            notification_tool(Some("Claude needs your permission to use mcp__x__y.")),
            "mcp__x__y"
        );
        assert_eq!(
            notification_tool(Some("Claude needs your attention")),
            "approval"
        );
        assert_eq!(notification_tool(None), "approval");
        let stop = |v: Value| has_background_tasks(&json!({"background_tasks": v}));
        assert!(stop(json!([{"id": "a"}])) && stop(json!({"a": 1})) && stop(json!(2)));
        assert!(!stop(json!([])) && !stop(json!({})) && !stop(json!(0)) && !stop(json!(null)));
        assert!(!has_background_tasks(&json!({})));
    }

    #[test]
    fn background_agents_outlive_the_short_staleness_rules_but_not_a_lost_report() {
        const MIN: u64 = 60_000;
        let mut a = ClaudeAdapter::new();
        hook(&mut a, json!({"hook_event_name": "UserPromptSubmit"}), 0);
        hook(
            &mut a,
            json!({"hook_event_name": "PreToolUse", "tool_name": "Agent", "tool_use_id": "t1",
                   "tool_input": {"description": "slow", "run_in_background": true}}),
            1000,
        );
        hook(
            &mut a,
            json!({"hook_event_name": "Stop", "background_tasks": [{"id": "x"}]}),
            2000,
        );
        // a prompt 11 minutes later keeps the background agent (a foreground one would be dropped)
        hook(
            &mut a,
            json!({"hook_event_name": "UserPromptSubmit", "prompt": "hi"}),
            12 * MIN,
        );
        assert_eq!(a.subs.len(), 1);
        hook(&mut a, json!({"hook_event_name": "Stop"}), 13 * MIN);
        assert_eq!(a.effective().0, Status::Run);
        // a tick within the two hours changes nothing; after them the lost report is given up on
        assert!(a.observe(&Observation::Tick { now: 90 * MIN }).is_empty());
        let d = a.observe(&Observation::Tick { now: 125 * MIN });
        assert_eq!(d[0].status, Some(Status::Done));
        // and a pending `background_tasks` hold expires the same way
        let mut b = ClaudeAdapter::new();
        hook(
            &mut b,
            json!({"hook_event_name": "Stop", "background_tasks": [1]}),
            0,
        );
        assert_eq!(b.effective().0, Status::Run);
        let d = b.observe(&Observation::Tick { now: 121 * MIN });
        assert_eq!(d[0].status, Some(Status::Done));
    }

    #[test]
    fn a_sub_agents_own_session_events_do_not_touch_the_lead() {
        let mut a = ClaudeAdapter::new();
        hook(&mut a, json!({"hook_event_name": "UserPromptSubmit"}), 0);
        for event in ["SessionStart", "SessionEnd", "UserPromptSubmit"] {
            hook(
                &mut a,
                json!({"hook_event_name": event, "agent_id": "ag", "source": "startup"}),
                1,
            );
        }
        assert_eq!(a.effective().0, Status::Run);
        assert!(a.subs.is_empty());
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
