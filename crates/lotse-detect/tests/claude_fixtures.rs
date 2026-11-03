//! Fixture-driven tests for the Claude hook adapter, through the public `Engine` API.
//!
//! Each file in `integrations/claude/fixtures/` is one scenario: a list of hook payloads (as
//! `lotse-hook` forwards them) with what the engine must report after each. Every step names the
//! status-table row it exercises (`row`); the last test asserts that every row of the table in
//! `claude.rs` is covered by at least one step.
//!
//! The fixtures are authored from the Claude Code hooks reference (see `lotse_detect::claude`), not
//! recorded from a live agent: tests never run a real agent.

use lotse_detect::{Engine, HookObservation, Observation};
use lotse_proto::{AgentKind, DetectSource, SessionId};
use serde_json::{Value, json};
use std::collections::BTreeSet;
use std::path::PathBuf;

/// Rows of the status table in `claude.rs` that fixtures must cover.
const ROWS: &[&str] = &[
    "session_start.startup_resume_clear",
    "session_start.compact",
    "prompt",
    "prompt.drops_finished_subs",
    "pre_tool.lead",
    "post_tool.lead",
    "post_tool_failure.lead",
    "perm_request.lead",
    "perm.cleared_by_post_tool",
    "perm.cleared_by_next_pre_tool",
    "perm.parallel_tool_keeps_prompt",
    "stop",
    "stop_failure.lead",
    "subagent_start",
    "tool.sub",
    "perm_request.sub",
    "stop_failure.sub",
    "subagent_stop",
    "session_end",
    "derived.stop_keeps_run_while_subs_run",
    "derived.sub_perm_is_session_perm",
    "derived.fail_over_running_sub",
    "ignored",
    "late.unknown_sub",
    // Agent tool, background agents, task notifications, permission notifications
    "agent_tool.pre",
    "agent_tool.post_foreground",
    "agent_tool.post_async",
    "agent_tool.post_failure",
    "subagent_start.adopts_tool_row",
    "stop.sub",
    "stop.background_tasks",
    "derived.background_keeps_run",
    "task_notification.prompt",
    "task_notification.transcript",
    "task_notification.failure",
    "task_notification.unknown",
    "notification.permission_prompt",
    "notification.cleared_by_tool",
];

fn fixtures_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../integrations/claude/fixtures")
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
    let mut engine = Engine::with_default_adapters(SessionId::new("s1"), AgentKind::ClaudeCode);
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
            agent: AgentKind::ClaudeCode,
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
fn fixture_payloads_use_only_documented_event_names() {
    // The names `lotse-hook` can receive from the installed hooks (integrations/claude/hooks.json),
    // plus a few events the adapter must ignore.
    const KNOWN: &[&str] = &[
        "SessionStart",
        "SessionEnd",
        "UserPromptSubmit",
        "PreToolUse",
        "PostToolUse",
        "PostToolUseFailure",
        "PermissionRequest",
        "Stop",
        "StopFailure",
        "SubagentStart",
        "SubagentStop",
        "Notification",
        "PreCompact",
        "PostCompact",
        "TeammateIdle",
        // built by the daemon from a transcript line, never sent by Claude Code
        "TaskNotification",
    ];
    for (name, scenario) in load_all() {
        for step in scenario["steps"].as_array().unwrap() {
            // The `ignored` row also holds malformed payloads (no or empty event name).
            if let Some(event) = step["payload"]["hook_event_name"]
                .as_str()
                .filter(|e| !e.is_empty())
            {
                assert!(KNOWN.contains(&event), "{name}: unknown event {event}");
            }
        }
    }
}
