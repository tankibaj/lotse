//! Fixture-driven tests for the Codex hook adapter, through the public `Engine` API.
//!
//! Each file in `integrations/codex/fixtures/` is one scenario: a list of hook payloads (as
//! `lotse-hook` forwards them, after its data minimisation) with what the engine must report after
//! each. Every step names the status-table row it exercises (`row`); one test asserts that every
//! row of the table in `codex.rs` is covered by at least one step.
//!
//! The payloads are shaped like Codex 0.162.1's (field names read from the binary, event
//! semantics from Orca's Codex hook handling), authored, not recorded: tests never run a real agent.

use lotse_detect::{Engine, HookObservation, Observation, ScreenObservation};
use lotse_proto::{AgentKind, DetectSource, SessionId, Status};
use serde_json::{Value, json};
use std::collections::BTreeSet;
use std::path::PathBuf;

/// Rows of the status table in `codex.rs` that fixtures must cover.
const ROWS: &[&str] = &[
    "session_start.startup_resume_clear_fork",
    "session_start.compact",
    "session_start.child",
    "prompt",
    "prompt.child",
    "prompt.drops_finished_subs",
    "pre_tool.lead",
    "post_tool.lead",
    "perm_request.lead",
    "perm.cleared_by_post_tool",
    "perm.cleared_by_next_pre_tool",
    "user_input_tool.lead",
    "stop",
    "stop.child",
    "interrupt",
    "interrupt.child",
    "subagent_start",
    "tool.sub",
    "perm_request.sub",
    "subagent_stop",
    "derived.stop_keeps_run_while_subs_run",
    "ignored",
    "late.unknown_sub",
];

fn fixtures_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../integrations/codex/fixtures")
}

fn load_all() -> Vec<(String, Value)> {
    let mut files: Vec<_> = std::fs::read_dir(fixtures_dir())
        .expect("fixtures dir")
        .map(|e| e.unwrap().path())
        .filter(|p| p.extension().is_some_and(|e| e == "json"))
        .collect();
    files.sort();
    assert!(!files.is_empty(), "no fixtures found");
    files
        .into_iter()
        .map(|p| {
            let text = std::fs::read_to_string(&p).unwrap();
            let name = p.file_name().unwrap().to_string_lossy().into_owned();
            let value = serde_json::from_str(&text).unwrap_or_else(|e| panic!("{name}: {e}"));
            (name, value)
        })
        .collect()
}

/// Run one scenario; returns the rows its steps exercise.
fn run_scenario(name: &str, scenario: &Value) -> BTreeSet<String> {
    let mut engine = Engine::with_default_adapters(SessionId::new("s1"), AgentKind::Codex);
    let mut rows = BTreeSet::new();
    for (i, step) in scenario["steps"]
        .as_array()
        .expect("steps")
        .iter()
        .enumerate()
    {
        let at = format!("{name} step {i} ({})", step["payload"]["hook_event_name"]);
        let row = step["row"]
            .as_str()
            .unwrap_or_else(|| panic!("{at}: no row"));
        assert!(ROWS.contains(&row), "{at}: unknown row {row:?}");
        rows.insert(row.to_owned());

        let update = engine.observe(Observation::Hook(HookObservation {
            agent: AgentKind::Codex,
            payload: step["payload"].clone(),
            at: step["at"].as_u64().unwrap_or_else(|| panic!("{at}: no at")),
        }));

        let expect_update = step["update"]
            .as_bool()
            .unwrap_or_else(|| panic!("{at}: no update"));
        assert_eq!(
            update.is_some(),
            expect_update,
            "{at}: update presence, got {update:?}"
        );
        assert_eq!(
            json!(engine.status()),
            step["status"],
            "{at}: engine status"
        );
        if let Some(expected) = step.get("sub_agents") {
            assert_eq!(&json!(engine.sub_agents()), expected, "{at}: sub-agents");
        }
        if let Some(update) = update {
            assert_eq!(update.source, DetectSource::Hook, "{at}: source");
            assert_eq!(json!(update.status), step["status"], "{at}: update status");
            if let Some(expected) = step.get("perm") {
                assert_eq!(&json!(update.perm), expected, "{at}: perm");
            }
            if let Some(expected) = step.get("running_since") {
                assert_eq!(
                    &json!(update.running_since),
                    expected,
                    "{at}: running_since"
                );
            }
        }
    }
    rows
}

#[test]
fn every_fixture_scenario_passes() {
    for (name, scenario) in load_all() {
        run_scenario(&name, &scenario);
    }
}

#[test]
fn fixtures_cover_every_row_of_the_status_table() {
    let mut covered = BTreeSet::new();
    for (name, scenario) in load_all() {
        covered.extend(run_scenario(&name, &scenario));
    }
    let missing: Vec<_> = ROWS.iter().filter(|r| !covered.contains(**r)).collect();
    assert!(
        missing.is_empty(),
        "rows without a fixture step: {missing:?}"
    );
}

#[test]
fn fixture_payloads_use_only_event_names_codex_has() {
    // The events of `lotse_hook::codex_home::EVENTS`, plus ones the adapter must ignore (all of
    // them in the Codex 0.162.1 binary, except `Notification`, which Codex does not have: the
    // adapter must ignore a name it does not know).
    const KNOWN: &[&str] = &[
        "SessionStart",
        "UserPromptSubmit",
        "PreToolUse",
        "PermissionRequest",
        "PostToolUse",
        "SubagentStart",
        "SubagentStop",
        "Stop",
        "Interrupt",
        "PreCompact",
        "PostCompact",
        "Notification",
    ];
    for (name, scenario) in load_all() {
        for step in scenario["steps"].as_array().unwrap() {
            if let Some(event) = step["payload"]["hook_event_name"]
                .as_str()
                .filter(|e| !e.is_empty())
            {
                assert!(KNOWN.contains(&event), "{name}: unknown event {event}");
            }
        }
    }
}

fn hook(event: &str, at: u64) -> Observation {
    Observation::Hook(HookObservation {
        agent: AgentKind::Codex,
        payload: json!({"hook_event_name": event}),
        at,
    })
}

fn working_screen(at: u64) -> Observation {
    Observation::Screen(ScreenObservation {
        rows: vec![
            "• Working (3s • esc to interrupt)".into(),
            String::new(),
            "› ".into(),
        ],
        at,
    })
}

#[test]
fn a_hook_status_wins_over_the_screen_for_codex() {
    let mut engine = Engine::with_default_adapters(SessionId::new("s1"), AgentKind::Codex);
    // before any hook the screen manifest is the only source
    let update = engine.observe(working_screen(1000)).expect("screen update");
    assert_eq!(
        (update.status, update.source),
        (Status::Run, DetectSource::Screen)
    );

    // the first hook takes over, and the screen still saying "working" does not take it back
    let update = engine.observe(hook("Stop", 2000)).expect("hook update");
    assert_eq!(
        (update.status, update.source),
        (Status::Done, DetectSource::Hook)
    );
    assert_eq!(engine.observe(working_screen(3000)), None);
    assert_eq!(engine.status(), Status::Done);
}
