//! The Claude mod, daemon side (`lotse-hook mod-bridge` / `mod-status` talk to it).
//!
//! - [`ModAttachments`]: which connection attached for which session (`Request::ModAttach`).
//!   `Event::ModCommand` is delivered to that connection only, never broadcast.
//! - [`features`]: what a status line carries besides status: context share, rate limits, todo
//!   progress. Status, permission asks and sub-agents are `lotse_detect::claude_mod`'s.
//! - [`pricing`]: the daemon's price table for the mod's per-agent tokens.
//!
//! Only a connection that said hello as [`MOD_CLIENT`] attaches or sends status lines; the same user
//! can still pose as one, so this is defence in depth.

use crate::hub::ClientTx;
use crate::state::State;
use crate::transcript::parse_iso_utc_ms;
use crate::usage::{Counts, price_for};
use lotse_detect::claude_mod::{Pricing, TokenCounts};
use lotse_proto::{Event, Frame, Message, ModCommand, RateLimit, SessionId, Todo, UnixMs};
use serde_json::Value;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

/// Hello `client` of `lotse-hook mod-bridge` and `mod-status`.
pub const MOD_CLIENT: &str = "mod";

const MAX_RATE_LIMITS: usize = 8;
const MAX_KIND_CHARS: usize = 32;
/// Below this a `resets_at` is in seconds (milliseconds will not be this small until the year 5138).
const SECONDS_BELOW: u64 = 100_000_000_000;

/// The connection each session's mod attached with.
#[derive(Default)]
pub struct ModAttachments {
    by_session: Mutex<HashMap<SessionId, Arc<ClientTx>>>,
}

impl ModAttachments {
    fn lock(&self) -> std::sync::MutexGuard<'_, HashMap<SessionId, Arc<ClientTx>>> {
        self.by_session.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// `client` receives the commands of `session` from now on (a second attach replaces the first).
    pub fn attach(&self, session: SessionId, client: Arc<ClientTx>) {
        self.lock().insert(session, client);
    }

    /// Queue `command` for the connection attached to `session`, and for no other. `false` when none
    /// is attached or it is gone.
    pub fn send(&self, session: &SessionId, command: ModCommand) -> bool {
        let Some(client) = self.lock().get(session).cloned() else {
            return false;
        };
        let event = Event::ModCommand {
            session_id: session.clone(),
            command,
        };
        let sent = client.send_frame(&Frame::Control(Message::Event(event)));
        if !sent {
            self.forget_client(client.id);
        }
        sent
    }

    /// A connection ended.
    pub fn forget_client(&self, client_id: u64) {
        self.lock().retain(|_, c| c.id != client_id);
    }

    /// A session ended.
    pub fn forget_session(&self, session: &SessionId) {
        self.lock().remove(session);
    }
}

impl State {
    /// Ask the mod of `session` to do something (M4: submit a prompt, answer a permission prompt).
    /// `false` when no mod is attached, so the caller takes the fallback (keystrokes).
    pub fn send_mod_command(&self, session: &SessionId, command: ModCommand) -> bool {
        self.mods().send(session, command)
    }
}

/// `Request::ModAttach`.
pub(crate) fn attach(
    state: &State,
    client: &Arc<ClientTx>,
    session: SessionId,
) -> lotse_proto::Response {
    use lotse_proto::{ErrorCode, ProtoError, Response};
    let refuse = |code, message: &str| {
        Response::Error(ProtoError {
            code,
            message: message.into(),
        })
    };
    if client.name != MOD_CLIENT {
        return refuse(ErrorCode::InvalidRequest, "only the Claude mod attaches");
    }
    if state.session(&session).is_none() {
        return refuse(ErrorCode::NotFound, "session not found");
    }
    state.mods().attach(session, client.clone());
    Response::Ok
}

/// What a status line says besides status. `None` / absent: the line did not say.
#[derive(Debug, Default, PartialEq)]
pub struct Features {
    /// Share of the context window in use, 0–100. **Unverified:** the scale of `usage.context.percent`.
    pub context_pct: Option<u8>,
    /// All of the account's rate-limit windows (an empty list: none reported).
    pub rate_limits: Option<Vec<RateLimit>>,
    /// `{"t":"todo","done":n,"total":m}`: the shape M2 asks the mod for (the mod sends none yet).
    pub todo: Option<Todo>,
}

fn percent(v: &Value) -> Option<u8> {
    let p = v.as_f64()?;
    p.is_finite().then(|| p.clamp(0.0, 100.0).round() as u8)
}

/// Seconds, milliseconds or an ISO timestamp.
fn resets_at(v: &Value) -> Option<UnixMs> {
    match v {
        Value::Number(n) => {
            let n = n.as_u64()?;
            Some(if n < SECONDS_BELOW { n * 1000 } else { n })
        }
        Value::String(s) => parse_iso_utc_ms(s),
        _ => None,
    }
}

fn rate_limit(entry: &Value) -> Option<RateLimit> {
    let kind = entry.get("kind")?.as_str().filter(|k| !k.is_empty())?;
    Some(RateLimit {
        kind: kind.chars().take(MAX_KIND_CHARS).collect(),
        pct: percent(entry.get("pct")?)?,
        resets_at: resets_at(entry.get("resets_at")?)?,
    })
}

/// The features of one status line (the payload `lotse-hook` forwarded).
pub fn features(line: &Value) -> Features {
    let todo = (line.get("t").and_then(Value::as_str) == Some("todo"))
        .then(|| {
            let count = |k: &str| u32::try_from(line.get(k)?.as_u64()?).ok();
            let (done, total) = (count("done")?, count("total")?);
            Some(Todo {
                done: done.min(total),
                total,
            })
        })
        .flatten();
    Features {
        context_pct: line.get("context_pct").and_then(percent),
        rate_limits: line
            .get("rate_limits")
            .and_then(Value::as_array)
            .map(|all| {
                all.iter()
                    .filter_map(rate_limit)
                    .take(MAX_RATE_LIMITS)
                    .collect()
            }),
        todo,
    }
}

/// The daemon's price table, for the cost of a sub-agent's tokens.
pub fn pricing() -> Pricing {
    Arc::new(|model: &str, c: TokenCounts| {
        let price = price_for(model)?;
        let micros = price.cost(Counts {
            input: c.input,
            output: c.output,
            cache_read: c.cache_read,
            cache_write: c.cache_write,
            cache_write_1h: 0,
        });
        Some(micros as f64 / 1e12)
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hub::Hub;
    use lotse_proto::read_frame;
    use serde_json::json;

    #[test]
    fn features_read_the_heartbeat_and_clamp_what_is_out_of_range() {
        let f = features(&json!({
            "t": "heartbeat", "context_pct": 41.6,
            "rate_limits": [
                {"kind": "five_hour", "pct": 61, "resets_at": 1_800_000_000},
                {"kind": "seven_day", "pct": 250, "resets_at": 1_800_000_000_000u64},
                {"kind": "iso", "pct": 1, "resets_at": "2026-11-03T10:00:00.000Z"},
                {"kind": "", "pct": 1, "resets_at": 1},
                {"kind": "no-time", "pct": 1},
                {"kind": "bad-time", "pct": 1, "resets_at": "soon"},
                "junk",
            ],
        }));
        assert_eq!(f.context_pct, Some(42));
        let limits = f.rate_limits.unwrap();
        assert_eq!(limits.len(), 3);
        assert_eq!(
            (limits[0].pct, limits[0].resets_at),
            (61, 1_800_000_000_000)
        );
        assert_eq!(limits[1].pct, 100);
        assert_eq!(limits[2].resets_at, 1_793_700_000_000);
    }

    #[test]
    fn a_line_without_the_fields_says_nothing_and_an_empty_list_says_none() {
        assert_eq!(features(&json!({"t": "heartbeat"})), Features::default());
        assert_eq!(
            features(&json!({"t": "heartbeat", "rate_limits": []})).rate_limits,
            Some(vec![])
        );
        assert_eq!(features(&json!({"context_pct": "high"})).context_pct, None);
    }

    #[test]
    fn todo_lines_give_progress_never_more_done_than_total() {
        let todo = |v| features(&v).todo;
        assert_eq!(
            todo(json!({"t": "todo", "done": 2, "total": 5})),
            Some(Todo { done: 2, total: 5 })
        );
        assert_eq!(
            todo(json!({"t": "todo", "done": 9, "total": 5})),
            Some(Todo { done: 5, total: 5 })
        );
        assert_eq!(todo(json!({"t": "todo", "done": 1})), None);
        assert_eq!(todo(json!({"t": "tool", "done": 1, "total": 2})), None);
    }

    #[test]
    fn the_price_table_prices_known_models_only() {
        let price = pricing();
        let counts = TokenCounts {
            input: 1_000_000,
            ..TokenCounts::default()
        };
        // Sonnet 5.5: $2 per million input tokens
        assert!((price("claude-sonnet-5-5", counts).unwrap() - 2.0).abs() < 1e-9);
        assert_eq!(price("local-llama", counts), None);
    }

    fn events(rx: &mut tokio::sync::mpsc::UnboundedReceiver<crate::hub::Bytes>) -> Vec<Event> {
        let mut out = Vec::new();
        while let Ok(bytes) = rx.try_recv() {
            if let Ok(Some(Frame::Control(Message::Event(e)))) = read_frame(&mut &bytes[..]) {
                out.push(e);
            }
        }
        out
    }

    #[test]
    fn a_command_goes_to_the_attached_connection_and_to_no_other() {
        let hub = Hub::default();
        let (mod_a, mut rx_a) = hub.register("mod".into());
        let (mod_b, mut rx_b) = hub.register("mod".into());
        let (_app, mut rx_app) = hub.register("app".into());
        let attached = ModAttachments::default();
        let (a, b) = (SessionId::new("s-a"), SessionId::new("s-b"));
        attached.attach(a.clone(), mod_a.clone());
        attached.attach(b.clone(), mod_b);

        assert!(attached.send(&a, ModCommand::Ping));
        assert_eq!(
            events(&mut rx_a),
            [Event::ModCommand {
                session_id: a.clone(),
                command: ModCommand::Ping
            }]
        );
        assert!(events(&mut rx_b).is_empty());
        assert!(events(&mut rx_app).is_empty());

        // a connection that ended is forgotten, and so is a closed session
        attached.forget_client(mod_a.id);
        assert!(!attached.send(&a, ModCommand::Ping));
        attached.forget_session(&b);
        assert!(!attached.send(&b, ModCommand::Ping));
        assert!(!attached.send(&SessionId::new("s-never"), ModCommand::Ping));
    }

    #[test]
    fn a_new_attach_replaces_the_old_connection() {
        let hub = Hub::default();
        let (old, mut rx_old) = hub.register("mod".into());
        let (new, mut rx_new) = hub.register("mod".into());
        let attached = ModAttachments::default();
        let s = SessionId::new("s");
        attached.attach(s.clone(), old);
        attached.attach(s.clone(), new);
        assert!(attached.send(&s, ModCommand::Ping));
        assert!(events(&mut rx_old).is_empty());
        assert_eq!(events(&mut rx_new).len(), 1);
    }
}
