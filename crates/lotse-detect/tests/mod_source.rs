//! The Claude mod as a detection source: `HookEvent{payload.kind:"lotse_mod"}` lines (the shapes
//! `integrations/claude/mod/register.js` sends, wrapped by `lotse-hook mod-bridge`) in, merged status out.

use lotse_detect::{Engine, HookObservation, Observation, StatusUpdate};
use lotse_proto::{AgentKind, DetectSource, SessionId, Status};
use serde_json::{Value, json};

const BEAT_MS: u64 = 10_000;

fn engine() -> Engine {
    Engine::with_default_adapters(SessionId::new("s1"), AgentKind::ClaudeCode)
}

fn mod_line(mut line: Value, at: u64) -> Observation {
    line["kind"] = "lotse_mod".into();
    line["lotse_source"] = "mod".into();
    Observation::Hook(HookObservation {
        agent: AgentKind::ClaudeCode,
        payload: line,
        at,
    })
}

fn hook(payload: Value, at: u64) -> Observation {
    Observation::Hook(HookObservation {
        agent: AgentKind::ClaudeCode,
        payload,
        at,
    })
}

fn agents(rows: Value) -> Value {
    json!({"t": "heartbeat", "ts": 1, "agents": rows})
}

fn perm(agent: &str, tool: &str, summary: &str) -> Value {
    json!({"t": "perm", "agent_id": agent, "tool": tool, "summary": summary})
}

fn tool(agent: &str, tool: &str, summary: &str) -> Value {
    json!({"t": "tool", "agent_id": agent, "tool": tool, "summary": summary})
}

fn row(id: &str, status: &str) -> Value {
    json!({"id": id, "parent": null, "type": "general-purpose", "description": format!("task {id}"), "status": status, "name": null})
}

fn up(e: &mut Engine, obs: Observation) -> StatusUpdate {
    e.observe(obs).expect("an update")
}

#[test]
fn a_mod_permission_ask_beats_the_hooks_run_and_names_its_request() {
    let mut e = engine();
    let u = up(
        &mut e,
        hook(json!({"hook_event_name": "UserPromptSubmit"}), 1),
    );
    assert_eq!((u.status, u.source), (Status::Run, DetectSource::Hook));
    let u = up(&mut e, mod_line(perm("a1", "Bash", "rm -rf build"), 2));
    assert_eq!((u.status, u.source), (Status::Perm, DetectSource::Mod));
    let request = u.perm.expect("the request");
    assert_eq!(
        (request.tool.as_str(), request.summary.as_str()),
        ("Bash", "rm -rf build")
    );
}

#[test]
fn the_ask_ends_with_the_next_tool_line_of_that_agent() {
    let mut e = engine();
    up(
        &mut e,
        hook(json!({"hook_event_name": "UserPromptSubmit"}), 1),
    );
    up(&mut e, mod_line(perm("a1", "Bash", "ls"), 2));
    // another agent's tool call does not answer a1's prompt
    assert_eq!(e.observe(mod_line(tool("a2", "Read", "x"), 3)), None);
    assert_eq!(e.status(), Status::Perm);
    let u = up(&mut e, mod_line(tool("a1", "Bash", "ls"), 4));
    assert_eq!(
        (u.status, u.source, u.perm),
        (Status::Run, DetectSource::Hook, None)
    );
}

#[test]
fn the_ask_ends_with_a_real_post_tool_use_or_stop_hook() {
    for end in ["PostToolUse", "Stop", "UserPromptSubmit", "SessionEnd"] {
        let mut e = engine();
        up(
            &mut e,
            hook(json!({"hook_event_name": "UserPromptSubmit"}), 1),
        );
        up(&mut e, mod_line(perm("a1", "Bash", "ls"), 2));
        let u = up(&mut e, hook(json!({"hook_event_name": end}), 3));
        assert_ne!(u.status, Status::Perm, "{end} answers the prompt");
        assert_eq!(u.perm, None);
    }
}

#[test]
fn a_notification_hook_does_not_end_the_ask() {
    let mut e = engine();
    up(&mut e, mod_line(perm("a1", "Bash", "ls"), 1));
    e.observe(hook(
        json!({"hook_event_name": "Notification", "notification_type": "permission_prompt"}),
        2,
    ));
    assert_eq!(e.status(), Status::Perm);
}

#[test]
fn heartbeat_agents_become_sub_agents_with_activity_tokens_and_model() {
    let mut e = engine();
    let u = up(
        &mut e,
        mod_line(
            agents(
                json!([row("a1", "running"), {"id": "a2", "parent": "a1", "type": "Explore", "status": "completed"}]),
            ),
            1,
        ),
    );
    let subs = u.sub_agents.expect("the list");
    assert_eq!(subs.len(), 2);
    assert_eq!(
        (subs[0].name.as_str(), subs[0].status),
        ("task a1", Status::Run)
    );
    assert_eq!(
        (subs[1].name.as_str(), subs[1].status),
        ("Explore", Status::Done)
    );
    assert_eq!(subs[1].parent.as_ref().map(|p| p.as_str()), Some("a1"));

    let u = up(&mut e, mod_line(tool("a1", "Read", "src/db/migrations"), 2));
    assert_eq!(
        u.sub_agents.unwrap()[0].activity.as_deref(),
        Some("Read src/db/migrations")
    );
    let u = up(
        &mut e,
        mod_line(
            json!({"t": "turn", "agent_id": "a1", "model": "claude-sonnet-5-5", "input": 100, "output": 20, "cache_read": 1000, "cache_creation": 5}),
            3,
        ),
    );
    let first = &u.sub_agents.unwrap()[0];
    assert_eq!(first.tokens, Some(1125));
    assert_eq!(first.model.as_deref(), Some("claude-sonnet-5-5"));
    let u = up(
        &mut e,
        mod_line(
            json!({"t": "turn", "agent_id": "a1", "input": 10, "output": 5}),
            4,
        ),
    );
    assert_eq!(u.sub_agents.unwrap()[0].tokens, Some(1140), "turns add up");
}

#[test]
fn the_daemons_price_table_prices_a_sub_agents_tokens() {
    use lotse_detect::claude_mod::{Pricing, TokenCounts};
    use std::sync::Arc;
    let mut e = engine();
    let pricing: Pricing = Arc::new(|model: &str, c: TokenCounts| {
        (model == "claude-sonnet-5-5").then_some(c.output as f64 * 0.001)
    });
    e.set_pricing(&pricing);
    up(&mut e, mod_line(agents(json!([row("a1", "running")])), 1));
    let turn = |model: &str, at| {
        mod_line(
            json!({"t": "turn", "agent_id": "a1", "model": model, "output": 500}),
            at,
        )
    };
    let u = up(&mut e, turn("claude-sonnet-5-5", 2));
    assert_eq!(u.sub_agents.unwrap()[0].cost_usd, Some(0.5));
    let u = up(&mut e, turn("some-local-model", 3));
    assert_eq!(u.sub_agents.unwrap()[0].cost_usd, None, "no price, no cost");
}

#[test]
fn the_sub_agent_of_a_mod_ask_shows_perm_on_its_row_and_the_session() {
    let mut e = engine();
    up(
        &mut e,
        mod_line(
            agents(json!([row("a1", "running"), row("a2", "running")])),
            1,
        ),
    );
    let u = up(&mut e, mod_line(perm("a2", "Write", "notes.md"), 2));
    assert_eq!(u.status, Status::Perm);
    let subs = u.sub_agents.unwrap();
    assert_eq!(
        (subs[0].status, subs[1].status),
        (Status::Run, Status::Perm)
    );
    let u = up(&mut e, mod_line(tool("a2", "Write", "notes.md"), 3));
    assert_eq!(u.sub_agents.unwrap()[1].status, Status::Run);
}

#[test]
fn the_mod_list_beats_the_hooks_and_a_lost_heartbeat_gives_them_back() {
    let mut e = engine();
    up(
        &mut e,
        hook(
            json!({"hook_event_name": "PreToolUse", "tool_name": "Agent", "tool_use_id": "t1", "tool_input": {"description": "from hooks"}}),
            1,
        ),
    );
    let u = up(&mut e, mod_line(agents(json!([row("a1", "running")])), 2));
    assert_eq!(u.sub_agents.unwrap()[0].name, "task a1");
    up(&mut e, mod_line(perm("a1", "Bash", "ls"), 3));
    assert_eq!(e.status(), Status::Perm);

    // still alive 20 s later: nothing changes
    assert_eq!(
        e.observe(Observation::Tick {
            now: 3 + 2 * BEAT_MS
        }),
        None
    );
    // silent for longer than 2 beats: the mod's opinion is gone, the hooks' is back
    let u = up(
        &mut e,
        Observation::Tick {
            now: 3 + 3 * BEAT_MS,
        },
    );
    assert_eq!(
        (u.status, u.source, u.perm),
        (Status::Run, DetectSource::Hook, None)
    );
    assert_eq!(u.sub_agents.unwrap()[0].name, "from hooks");
}

#[test]
fn a_returning_heartbeat_takes_the_session_back() {
    let mut e = engine();
    up(&mut e, mod_line(agents(json!([row("a1", "running")])), 1));
    up(
        &mut e,
        Observation::Tick {
            now: 1 + 4 * BEAT_MS,
        },
    );
    assert!(e.sub_agents().is_empty());
    let u = up(
        &mut e,
        mod_line(agents(json!([row("a9", "running")])), 2 + 4 * BEAT_MS),
    );
    assert_eq!(u.sub_agents.unwrap()[0].name, "task a9");
}

#[test]
fn an_empty_agent_list_is_the_mods_answer_but_a_missing_one_is_no_opinion() {
    let mut e = engine();
    up(
        &mut e,
        hook(
            json!({"hook_event_name": "PreToolUse", "tool_name": "Agent", "tool_use_id": "t1", "tool_input": {"description": "from hooks"}}),
            1,
        ),
    );
    // `agent.list()` failed in the mod: the line has no `agents`
    assert_eq!(
        e.observe(mod_line(json!({"t": "heartbeat", "ts": 1}), 2)),
        None
    );
    assert_eq!(e.sub_agents()[0].name, "from hooks");
    let u = up(&mut e, mod_line(agents(json!([])), 3));
    assert_eq!(u.sub_agents, Some(vec![]));
}

#[test]
fn only_claude_code_has_a_mod_source() {
    let mut e = Engine::with_default_adapters(SessionId::new("s"), AgentKind::Codex);
    assert_eq!(e.observe(mod_line(perm("a1", "Bash", "ls"), 1)), None);
    assert_eq!(e.status(), Status::Unknown);
}

#[test]
fn junk_lines_change_nothing() {
    let mut e = engine();
    up(
        &mut e,
        hook(json!({"hook_event_name": "UserPromptSubmit"}), 1),
    );
    for junk in [
        json!({"t": "perm"}),
        json!({"t": "tool", "agent_id": 7}),
        json!({"t": "turn", "agent_id": "a1", "input": "many"}),
        json!({"t": "heartbeat", "agents": "no"}),
        json!({"t": "heartbeat", "agents": [1, null, {"status": "running"}]}),
        json!({"t": "who-knows"}),
    ] {
        e.observe(mod_line(junk, 2));
    }
    assert_eq!(e.status(), Status::Run);
    assert!(e.sub_agents().is_empty());
}

#[test]
fn a_flood_of_agents_is_bounded() {
    let mut e = engine();
    let rows: Vec<Value> = (0..1000)
        .map(|n| row(&format!("a{n}"), "running"))
        .collect();
    e.observe(mod_line(agents(Value::Array(rows)), 1));
    assert!(e.sub_agents().len() <= 64);
}
