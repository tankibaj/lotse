//! Runs the Claude mod (`register.js`) under Node against a fake `$` (Claude Code itself is never
//! started): the version gate, the bridge it spawns, what it sends over `lotse-hook mod-status`, the
//! heartbeat, and that its hooks never change what the engine would have done. Skipped, with a note on
//! stderr, when `node` is missing.

use lotse_hook::mod_bridge::MOD_SOURCE;
use serde_json::Value;
use std::process::Command;

fn node() -> Option<std::path::PathBuf> {
    std::env::var_os("PATH").and_then(|paths| {
        std::env::split_paths(&paths)
            .map(|d| d.join("node"))
            .find(|p| p.is_file())
    })
}

/// A fake `$` and `on`. `world(o)`: `o.version`, `o.env` (over the Lotse defaults), `o.chunks` (what
/// the bridge writes to stdout; it then stays alive unless `o.ends`), `o.tick` (ms per `clock.now`),
/// `o.runFails`.
const HARNESS: &str = r#"
import { register } from './register.mjs'
const hooks = {}
register((ev, a, b) => { hooks[ev] = b ?? a }, {})
const settle = () => new Promise(r => setTimeout(r, 40))
function world(o = {}) {
  const calls = { run: [], spawn: [], every: [] }
  let now = 1000
  const env = { LOTSE_SESSION_ID: 'a1', LOTSE_HOOK_BIN: '/bin/lh', ...(o.env ?? {}) }
  const $ = {
    env: { get: async name => env[name] },
    session: {
      version: async () => ({ version: o.version ?? '2.1.311' }),
      usage: async () => ({
        context: { percent: 42, window: 200000, tokens: 84000 },
        rateLimits: [{ kind: 'five_hour', percentUsed: 12.5, resetsAt: '2026-11-09T12:00:00Z' }],
        cost: { usd: 0.5 },
      }),
    },
    agent: { list: async () => [{ id: 'g1', type: 'general-purpose', description: 'Count', status: 'running', name: 'n' }] },
    clock: {
      now: async () => (now += o.tick ?? 0),
      sleep: async () => {},
      every: (ms, fn) => { calls.every.push({ ms, fn }); return () => {} },
    },
    process: {
      run: async (argv, init) => {
        calls.run.push({ argv, stdin: init?.stdin })
        if (o.runFails) throw new Error('boom')
        return { exitCode: 0, stdout: '', stderr: '' }
      },
      spawn: req => {
        calls.spawn.push(req)
        return (async function* () {
          for (const text of o.chunks ?? []) yield { stream: 'stdout', text }
          if (!o.ends || calls.spawn.length > 8) await new Promise(() => {})
        })()
      },
    },
  }
  const sent = () => calls.run.flatMap(c => (c.stdin ?? '').split('\n').filter(Boolean).map(l => JSON.parse(l)))
  return { $, calls, sent }
}
const start = ($, e = { cwd: '/p' }) => hooks['session.start']($, e, async x => ({ started: true }))
const out = v => { console.log(JSON.stringify(v)); process.exit(0) }
"#;

fn scenario(body: &str) -> Option<Value> {
    let Some(node) = node() else {
        eprintln!("skipped: node is not installed");
        return None;
    };
    let tmp = tempfile::tempdir().unwrap();
    std::fs::write(tmp.path().join("register.mjs"), MOD_SOURCE).unwrap();
    std::fs::write(
        tmp.path().join("driver.mjs"),
        format!("{HARNESS}\n{body}\n"),
    )
    .unwrap();
    let output = Command::new(node)
        .arg("driver.mjs")
        .current_dir(tmp.path())
        .env_clear()
        .env("PATH", "/usr/bin:/bin")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "node failed: {}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let text = String::from_utf8(output.stdout).unwrap();
    Some(
        serde_json::from_str(text.lines().last().unwrap())
            .unwrap_or_else(|e| panic!("{e}: {text}")),
    )
}

const ATTACHED: &str = r#"chunks: ['{"bridge":"attached"}\n']"#;

#[test]
fn the_mod_registers_its_hooks_and_nothing_else() {
    let Some(got) = scenario("out(Object.keys(hooks).sort())") else {
        return;
    };
    assert_eq!(
        got,
        serde_json::json!([
            "session.end",
            "session.start",
            "tool.call",
            "tool.check",
            "turn.complete"
        ])
    );
}

#[test]
fn below_claude_2_1_311_the_mod_does_nothing() {
    let Some(got) = scenario(
        r#"
        const w = world({ version: '2.1.310' })
        const r = await start(w.$)
        await settle()
        out({ r, spawn: w.calls.spawn.length, run: w.calls.run.length, every: w.calls.every.length })"#,
    ) else {
        return;
    };
    assert_eq!(
        got,
        serde_json::json!({"r": {"started": true}, "spawn": 0, "run": 0, "every": 0})
    );
}

#[test]
fn outside_a_lotse_pane_the_mod_does_nothing() {
    let Some(got) = scenario(
        r#"
        const w = world({ env: { LOTSE_SESSION_ID: '' } })
        await start(w.$)
        await settle()
        out({ spawn: w.calls.spawn.length, run: w.calls.run.length })"#,
    ) else {
        return;
    };
    assert_eq!(got, serde_json::json!({"spawn": 0, "run": 0}));
}

#[test]
fn from_claude_2_1_311_it_starts_the_bridge_with_a_hello_and_a_heartbeat_timer() {
    let Some(got) = scenario(
        r#"
        const w = world({ version: '2.1.311-dev.20260920.t1.sha1' })
        const r = await start(w.$)
        await settle()
        out({ r, spawn: w.calls.spawn.map(s => ({ argv: s.argv, hello: JSON.parse(s.input) })),
              every: w.calls.every.map(e => e.ms), run: w.calls.run.length })"#,
    ) else {
        return;
    };
    assert_eq!(got["r"], serde_json::json!({"started": true}));
    assert_eq!(
        got["spawn"][0]["argv"],
        serde_json::json!(["/bin/lh", "mod-bridge"])
    );
    assert_eq!(got["spawn"][0]["hello"]["t"], "hello");
    assert_eq!(got["spawn"][0]["hello"]["mod"], 1);
    assert_eq!(
        got["spawn"][0]["hello"]["claude"],
        "2.1.311-dev.20260920.t1.sha1"
    );
    assert_eq!(got["every"], serde_json::json!([10000]));
    assert_eq!(got["run"], 0, "nothing is sent before the bridge attached");
}

#[test]
fn attached_it_heartbeats_with_usage_and_agents_and_answers_a_ping() {
    let Some(got) = scenario(
        r#"
        const w = world({ chunks: ['{"bridge":"attached"}\n{"cm', 'd":"ping"}\n'] })
        await start(w.$)
        await settle()
        const timer = w.calls.every[0].fn
        timer()
        await settle()
        out({ argv: w.calls.run.map(c => c.argv), beats: w.sent().filter(l => l.t === 'heartbeat') })"#,
    ) else {
        return;
    };
    for argv in got["argv"].as_array().unwrap() {
        assert_eq!(argv, &serde_json::json!(["/bin/lh", "mod-status"]));
    }
    let beats = got["beats"].as_array().unwrap();
    assert_eq!(
        beats.len(),
        3,
        "after attach, after the ping, on the timer: {beats:?}"
    );
    let b = &beats[0];
    assert_eq!(b["context_pct"], 42);
    assert_eq!(b["context_window"], 200000);
    assert_eq!(b["cost_usd"], 0.5);
    assert_eq!(
        b["rate_limits"],
        serde_json::json!([{"kind": "five_hour", "pct": 12.5, "resets_at": "2026-11-09T12:00:00Z"}])
    );
    assert_eq!(
        b["agents"],
        serde_json::json!([{"id": "g1", "type": "general-purpose", "description": "Count", "status": "running", "name": "n"}])
    );
    assert!(b["ts"].is_number());
}

#[test]
fn no_heartbeat_and_no_events_while_the_bridge_is_not_attached() {
    let Some(got) = scenario(
        r#"
        const w = world({ chunks: [] })
        await start(w.$)
        await settle()
        w.calls.every[0].fn()
        await hooks['tool.call'](w.$, { tool: 'Bash', command: 'ls' }, async e => e)
        await hooks['turn.complete'](w.$, { usage: { model: 'm', input_tokens: 1 } }, async e => e)
        await settle()
        out({ run: w.calls.run.length })"#,
    ) else {
        return;
    };
    assert_eq!(got, serde_json::json!({"run": 0}));
}

#[test]
fn a_tool_call_is_reported_with_its_agent_and_a_short_summary_and_passes_through() {
    let body = format!(
        r#"
        const w = world({{ {ATTACHED} }})
        await start(w.$)
        await settle()
        const first = {{ ok: 1 }}
        const r1 = await hooks['tool.call'](w.$, {{ tool: 'Bash', command: 'ls -la', agentId: 'g1' }}, async e => first)
        const r2 = await hooks['tool.call'](w.$, {{ tool: 'Edit', file_path: '/a/b.rs', old_string: 'secret', new_string: 'x' }}, async e => ({{ ok: 2 }}))
        const r3 = await hooks['tool.call'](w.$, {{ tool: 'Bash', command: 'x'.repeat(500) }}, async e => e)
        await settle()
        out({{ same: r1 === first, r2, tools: w.sent().filter(l => l.t === 'tool') }})"#
    );
    let Some(got) = scenario(&body) else { return };
    assert_eq!(got["same"], true);
    assert_eq!(got["r2"], serde_json::json!({"ok": 2}));
    let tools = got["tools"].as_array().unwrap();
    assert_eq!(
        tools[0],
        serde_json::json!({"t": "tool", "agent_id": "g1", "tool": "Bash", "summary": "ls -la"})
    );
    // file contents of an edit never travel: the path only
    assert_eq!(
        tools[1],
        serde_json::json!({"t": "tool", "tool": "Edit", "summary": "/a/b.rs"})
    );
    assert_eq!(tools[2]["summary"].as_str().unwrap().chars().count(), 200);
}

#[test]
fn a_permission_ask_for_a_person_is_reported_and_the_verdict_is_untouched() {
    let body = format!(
        r#"
        const w = world({{ {ATTACHED} }})
        await start(w.$)
        await settle()
        const ask = {{ decision: 'ask' }}
        const r1 = await hooks['tool.check'](w.$, {{ tool: 'Bash', command: 'rm x', agentId: 'g1', approver: 'person' }}, async e => ask)
        await hooks['tool.check'](w.$, {{ tool: 'Bash', command: 'ls', approver: 'person' }}, async e => ({{ decision: 'allow' }}))
        await hooks['tool.check'](w.$, {{ tool: 'Bash', command: 'ls', approver: 'classifier' }}, async e => ({{ decision: 'ask' }}))
        await settle()
        out({{ same: r1 === ask, perms: w.sent().filter(l => l.t === 'perm') }})"#
    );
    let Some(got) = scenario(&body) else { return };
    assert_eq!(got["same"], true);
    assert_eq!(
        got["perms"],
        serde_json::json!([{"t": "perm", "agent_id": "g1", "tool": "Bash", "summary": "rm x"}])
    );
}

#[test]
fn a_finished_turn_reports_its_model_and_token_counts() {
    let body = format!(
        r#"
        const w = world({{ {ATTACHED} }})
        await start(w.$)
        await settle()
        const usage = {{ model: 'claude-sonnet-5-5', input_tokens: 10, output_tokens: 20, cache_read_input_tokens: 30, cache_creation_input_tokens: 40 }}
        await hooks['turn.complete'](w.$, {{ agentId: 'g1', usage }}, async e => e)
        await hooks['turn.complete'](w.$, {{ usage: null }}, async e => e)
        await settle()
        out(w.sent().filter(l => l.t === 'turn'))"#
    );
    let Some(got) = scenario(&body) else { return };
    assert_eq!(
        got,
        serde_json::json!([{"t": "turn", "agent_id": "g1", "model": "claude-sonnet-5-5",
            "input": 10, "output": 20, "cache_read": 30, "cache_creation": 40}])
    );
}

#[test]
fn a_failing_side_channel_never_changes_a_hooks_answer() {
    let body = format!(
        r#"
        const w = world({{ {ATTACHED}, runFails: true }})
        await start(w.$)
        await settle()
        const r = await hooks['tool.call'](w.$, {{ tool: 'Bash', command: 'ls' }}, async e => ({{ ok: 3 }}))
        await settle()
        out({{ r }})"#
    );
    let Some(got) = scenario(&body) else { return };
    assert_eq!(got, serde_json::json!({"r": {"ok": 3}}));
}

#[test]
fn a_bridge_that_keeps_dying_is_given_up_on_and_the_heartbeat_stops() {
    let Some(got) = scenario(
        r#"
        const w = world({ ends: true, chunks: [] })
        await start(w.$)
        await settle()
        await settle()
        w.calls.every[0].fn()
        await settle()
        out({ spawns: w.calls.spawn.length, run: w.calls.run.length })"#,
    ) else {
        return;
    };
    assert_eq!(got, serde_json::json!({"spawns": 5, "run": 0}));
}

#[test]
fn a_bridge_that_lived_a_while_is_restarted_without_limit() {
    let Some(got) = scenario(
        r#"
        const w = world({ ends: true, chunks: ['{"bridge":"attached"}\n'], tick: 20000 })
        await start(w.$)
        await settle()
        out({ many: w.calls.spawn.length > 5 })"#,
    ) else {
        return;
    };
    assert_eq!(got, serde_json::json!({"many": true}));
}

#[test]
fn junk_and_commands_the_mod_does_not_know_are_ignored() {
    let Some(got) = scenario(
        r#"
        const w = world({ chunks: ['junk\n{"cmd":"submit","text":"x"}\n{"bridge":"attached"}\n[1]\n'] })
        w.$.prompt = { submit: async () => { throw new Error('must not be called') } }
        await start(w.$)
        await settle()
        out({ beats: w.sent().filter(l => l.t === 'heartbeat').length })"#,
    ) else {
        return;
    };
    assert_eq!(got, serde_json::json!({"beats": 1}));
}

#[test]
fn session_end_stops_the_reporting() {
    let body = format!(
        r#"
        const w = world({{ {ATTACHED} }})
        await start(w.$)
        await settle()
        const r = await hooks['session.end'](w.$, {{ reason: 'exit' }}, async e => ({{ ended: true }}))
        const before = w.calls.run.length
        await hooks['tool.call'](w.$, {{ tool: 'Bash', command: 'ls' }}, async e => e)
        await settle()
        out({{ r, after: w.calls.run.length - before }})"#
    );
    let Some(got) = scenario(&body) else { return };
    assert_eq!(got, serde_json::json!({"r": {"ended": true}, "after": 0}));
}
