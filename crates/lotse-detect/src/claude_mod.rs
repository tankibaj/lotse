//! Claude Code mod adapter: the in-process module's live data. Source: [`DetectSource::Mod`], above
//! the hooks.
//!
//! # Where the events come from
//!
//! The mod (`integrations/claude/mod/register.js`) sends status lines through `lotse-hook mod-bridge` /
//! `mod-status`, which wrap each as a `HookEvent` whose payload holds `kind: "lotse_mod"` and the mod's
//! own fields; the daemon makes a [`HookObservation`] of it. Lines the adapter reads (`t`):
//!
//! | `t` | Fields | Effect |
//! |---|---|---|
//! | `hello`, `heartbeat` | `agents`: `[{id, parent, name, description, type, status}]` | the mod is alive; `agents` (when present) replaces the sub-agent list, an empty one means "none" |
//! | `tool` | `agent_id`, `tool`, `summary` | the agent's activity line (`Read src/db`); ends that agent's permission ask |
//! | `perm` | `agent_id`, `tool`, `summary` | the agent asks the person: session `Perm` (and `Perm` on the agent's row) |
//! | `turn` | `agent_id`, `model`, `input`, `output`, `cache_read`, `cache_creation` | adds to the agent's tokens, sets its model; ends its ask |
//!
//! # What the mod claims
//!
//! Only `Perm`, and only while an ask is open: the mod sees an ask the hooks miss (a background
//! agent's), but its lines are model steps, not turn boundaries, so it never claims `Run` or `Done`
//! (the hooks keep those, and a claim nothing could end would outrank them). An ask ends with the
//! next `tool` or `turn` line of the same agent, a real hook that ends a tool or a turn
//! (`PostToolUse`, `Stop`, `UserPromptSubmit`, ...; one that names an agent ends only that agent's
//! ask), or the end of the process. A fresh permission dialog on the screen beats all of this (engine merge rule).
//!
//! # Heartbeat
//!
//! The mod reports every 10 s. [`LOST_AFTER_MS`] without a line, [`Adapter::expired`] drops everything
//! the adapter said, and the engine falls back to hooks and screen. A later line brings the mod back.
//!
//! **Unverified** against a real Claude Code: the `status` words of `agent.list()` other than
//! `running` (G3 saw only that one), whether `agent.list()` holds the main agent, and the scale of
//! `usage.context.percent`. The mod probe notes are in the plan's gates.

use crate::adapter::{Adapter, Detection, Observation};
use lotse_proto::{
    AgentKind, DetectSource, PermRequest, SessionId, Status, SubAgent, SubAgentId, UnixMs,
};
use serde_json::Value;
use std::collections::HashMap;
use std::sync::Arc;

/// `payload.kind` of every status line of the mod.
pub const MOD_KIND: &str = "lotse_mod";

/// The mod reports every 10 s; a silence of two and a half beats means it is gone.
pub const LOST_AFTER_MS: UnixMs = 25_000;

const MAX_AGENTS: usize = 64;
const MAX_DETAILS: usize = 256;
const MAX_PERMS: usize = 16;
const ID_MAX_CHARS: usize = 128;
const NAME_MAX_CHARS: usize = 60;
const ACTIVITY_MAX_CHARS: usize = 120;
const SUMMARY_MAX_CHARS: usize = 160;

/// Hook events that end a tool call or a turn, and so any ask in it.
const ENDING_HOOKS: [&str; 7] = [
    "PostToolUse",
    "PostToolUseFailure",
    "UserPromptSubmit",
    "Stop",
    "StopFailure",
    "SubagentStop",
    "SessionEnd",
];

/// Token counts of an agent, as the mod's `turn` lines report them.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct TokenCounts {
    pub input: u64,
    pub output: u64,
    pub cache_read: u64,
    pub cache_write: u64,
}

impl TokenCounts {
    pub fn total(self) -> u64 {
        self.input
            .saturating_add(self.output)
            .saturating_add(self.cache_read)
            .saturating_add(self.cache_write)
    }
}

/// US dollars of `counts` of `model`, `None` when the model has no known price. The daemon owns the
/// price table, so it hands this in ([`crate::Engine::set_pricing`]).
pub type Pricing = Arc<dyn Fn(&str, TokenCounts) -> Option<f64> + Send + Sync>;

/// `true` for a status line of the mod.
pub fn is_mod_line(payload: &Value) -> bool {
    payload.get("kind").and_then(Value::as_str) == Some(MOD_KIND)
}

/// Called by `Engine::with_default_adapters`: the mod adapter for `AgentKind::ClaudeCode`.
pub fn default_adapter(_session: &SessionId, agent: AgentKind) -> Option<Box<dyn Adapter>> {
    (agent == AgentKind::ClaudeCode).then(|| Box::new(ModAdapter::default()) as Box<dyn Adapter>)
}

#[derive(Debug, Clone)]
struct Row {
    id: String,
    parent: Option<String>,
    name: String,
    status: Status,
}

#[derive(Debug, Clone, Default)]
struct Detail {
    activity: Option<String>,
    counts: TokenCounts,
    model: Option<String>,
}

#[derive(Default)]
pub struct ModAdapter {
    pricing: Option<Pricing>,
    /// When the last line came; `None` while the mod is not (or no longer) heard.
    last_seen: Option<UnixMs>,
    /// The list of the last heartbeat that carried one.
    rows: Option<Vec<Row>>,
    /// Per agent id: what `tool` and `turn` lines said.
    details: HashMap<String, Detail>,
    /// Open asks, oldest first: `(agent id, request)`.
    asks: Vec<(Option<String>, PermRequest)>,
}

fn text<'a>(line: &'a Value, key: &str) -> Option<&'a str> {
    line.get(key)
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
}

fn number(line: &Value, key: &str) -> u64 {
    line.get(key).and_then(Value::as_u64).unwrap_or(0)
}

fn clip(s: &str, max: usize) -> String {
    s.chars().take(max).collect()
}

/// The agent's status words (`agent.list()`); only `running` is verified against a real Claude Code.
fn map_status(word: &str) -> Status {
    match word.to_ascii_lowercase().as_str() {
        "running" | "waiting" | "pending" | "starting" | "queued" => Status::Run,
        "completed" | "complete" | "done" | "finished" | "succeeded" | "success" | "stopped"
        | "killed" | "cancelled" | "canceled" => Status::Done,
        "failed" | "error" | "errored" => Status::Fail,
        _ => Status::Unknown,
    }
}

fn parse_row(row: &Value) -> Option<Row> {
    let id = clip(text(row, "id")?, ID_MAX_CHARS);
    let name = ["name", "description", "type"]
        .into_iter()
        .find_map(|key| text(row, key))
        .map_or_else(|| id.clone(), |n| clip(n, NAME_MAX_CHARS));
    Some(Row {
        parent: text(row, "parent").map(|p| clip(p, ID_MAX_CHARS)),
        status: text(row, "status").map_or(Status::Unknown, map_status),
        name,
        id,
    })
}

impl ModAdapter {
    /// An adapter that prices sub-agent tokens with `pricing`.
    pub fn with_pricing(pricing: Pricing) -> Self {
        Self {
            pricing: Some(pricing),
            ..Self::default()
        }
    }

    fn forget(&mut self) {
        self.last_seen = None;
        self.rows = None;
        self.details.clear();
        self.asks.clear();
    }

    /// Ends the asks of `agent`; every ask when `agent` is `None` and `all`. `true` when one ended.
    fn end_asks(&mut self, agent: Option<&str>, all: bool) -> bool {
        let before = self.asks.len();
        self.asks
            .retain(|(who, _)| !(all || who.as_deref() == agent));
        self.asks.len() != before
    }

    fn detail(&mut self, agent: &str) -> Option<&mut Detail> {
        if !self.details.contains_key(agent) && self.details.len() >= MAX_DETAILS {
            return None;
        }
        Some(self.details.entry(agent.to_owned()).or_default())
    }

    fn sub_agents(&self) -> Option<Vec<SubAgent>> {
        let rows = self.rows.as_ref()?;
        Some(
            rows.iter()
                .map(|row| {
                    let detail = self.details.get(&row.id);
                    let asking = self
                        .asks
                        .iter()
                        .any(|(who, _)| who.as_deref() == Some(row.id.as_str()));
                    let mut sub = SubAgent::new(
                        SubAgentId::new(row.id.as_str()),
                        row.name.as_str(),
                        if asking { Status::Perm } else { row.status },
                    );
                    sub.parent = row.parent.as_deref().map(SubAgentId::new);
                    if let Some(d) = detail {
                        sub.activity.clone_from(&d.activity);
                        sub.tokens = Some(d.counts.total()).filter(|t| *t > 0);
                        sub.model.clone_from(&d.model);
                        sub.cost_usd = self
                            .pricing
                            .as_ref()
                            .zip(d.model.as_deref())
                            .and_then(|(price, model)| price(model, d.counts));
                    }
                    sub
                })
                .collect(),
        )
    }

    /// What the adapter says now: `Perm` with the newest open ask, else no claim.
    fn detection(&self, at: UnixMs) -> Vec<Detection> {
        let (status, perm) = match self.asks.last() {
            Some((_, request)) => (Status::Perm, Some(request.clone())),
            None => (Status::Unknown, None),
        };
        vec![Detection {
            source: DetectSource::Mod,
            status: Some(status),
            sub_agents: self.sub_agents(),
            perm,
            at,
        }]
    }

    fn on_line(&mut self, line: &Value, at: UnixMs) -> Vec<Detection> {
        self.last_seen = Some(at);
        let agent = text(line, "agent_id").map(|a| clip(a, ID_MAX_CHARS));
        match text(line, "t") {
            Some("heartbeat") => {
                if let Some(list) = line.get("agents").and_then(Value::as_array) {
                    self.rows = Some(list.iter().filter_map(parse_row).take(MAX_AGENTS).collect());
                }
            }
            Some("tool") => {
                self.end_asks(agent.as_deref(), false);
                if let (Some(agent), Some(tool)) = (agent.as_deref(), text(line, "tool")) {
                    let activity = match text(line, "summary") {
                        Some(summary) => clip(&format!("{tool} {summary}"), ACTIVITY_MAX_CHARS),
                        None => clip(tool, ACTIVITY_MAX_CHARS),
                    };
                    if let Some(d) = self.detail(agent) {
                        d.activity = Some(activity);
                    }
                }
            }
            Some("perm") => {
                let Some(tool) = text(line, "tool") else {
                    return Vec::new();
                };
                self.end_asks(agent.as_deref(), false);
                if self.asks.len() == MAX_PERMS {
                    self.asks.remove(0);
                }
                let summary = text(line, "summary").unwrap_or(tool);
                self.asks.push((
                    agent,
                    PermRequest {
                        tool: clip(tool, ID_MAX_CHARS),
                        summary: clip(summary, SUMMARY_MAX_CHARS),
                        always: None,
                        id: None,
                    },
                ));
            }
            Some("turn") => {
                self.end_asks(agent.as_deref(), false);
                if let Some(agent) = agent.as_deref()
                    && let Some(d) = self.detail(agent)
                {
                    let c = &mut d.counts;
                    c.input = c.input.saturating_add(number(line, "input"));
                    c.output = c.output.saturating_add(number(line, "output"));
                    c.cache_read = c.cache_read.saturating_add(number(line, "cache_read"));
                    c.cache_write = c.cache_write.saturating_add(number(line, "cache_creation"));
                    if let Some(model) = text(line, "model") {
                        d.model = Some(clip(model, ID_MAX_CHARS));
                    }
                }
            }
            _ => {}
        }
        self.detection(at)
    }

    /// A real hook: some of them end an ask the mod reported.
    fn on_hook(&mut self, payload: &Value, at: UnixMs) -> Vec<Detection> {
        let ends = text(payload, "hook_event_name").is_some_and(|e| ENDING_HOOKS.contains(&e));
        let agent = text(payload, "agent_id");
        if ends && self.end_asks(agent, agent.is_none()) {
            self.detection(at)
        } else {
            Vec::new()
        }
    }
}

impl Adapter for ModAdapter {
    fn name(&self) -> &'static str {
        "claude-mod"
    }

    fn agent(&self) -> AgentKind {
        AgentKind::ClaudeCode
    }

    fn source(&self) -> DetectSource {
        DetectSource::Mod
    }

    fn observe(&mut self, obs: &Observation) -> Vec<Detection> {
        match obs {
            Observation::Hook(h) if is_mod_line(&h.payload) => self.on_line(&h.payload, h.at),
            Observation::Hook(h) => self.on_hook(&h.payload, h.at),
            Observation::Process(p) if p.foreground.is_none() && self.last_seen.is_some() => {
                self.forget();
                vec![Detection {
                    source: DetectSource::Mod,
                    status: Some(Status::Unknown),
                    sub_agents: Some(Vec::new()),
                    perm: None,
                    at: p.at,
                }]
            }
            _ => Vec::new(),
        }
    }

    fn expired(&mut self, now: UnixMs) -> bool {
        let lost = self
            .last_seen
            .is_some_and(|seen| now.saturating_sub(seen) > LOST_AFTER_MS);
        if lost {
            self.forget();
        }
        lost
    }

    fn set_pricing(&mut self, pricing: Pricing) {
        self.pricing = Some(pricing);
    }
}
