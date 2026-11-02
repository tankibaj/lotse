//! PTY transcript replays for the screen and OSC 7501 adapters, through the public `Engine` API.
//!
//! Two kinds of fixture live in `integrations/manifests/fixtures/`:
//!
//! - `*.json` (authored): one session as steps (`out` PTY bytes, `tick`, `process`), each with the
//!   status the engine must report afterwards. `out` is fed to the OSC 7501 scanner and to a minimal
//!   screen model (`support::Screen`); the rendered rows go to the engine as `Observation::Screen`,
//!   as the daemon does once output goes quiet.
//! - `recorded/*.json` (recorded): real Codex PTY output with its timing, replayed the way the daemon
//!   samples (100 ms of quiet, at most every 500 ms while output streams, a tick per second), with the
//!   status sequence the engine must report. Provenance is in each file.
//!
//! Tests never run a real agent. `export_orca_recordings` (ignored) regenerates `recorded/` from
//! Orca's checkout.

mod support;

use lotse_detect::osc7501::{self, Osc7501Adapter, Osc7501Scanner};
use lotse_detect::screen::ScreenAdapter;
use lotse_detect::screen::manifest::ManifestSet;
use lotse_detect::{Engine, Observation, ProcessObservation, ScreenObservation, StatusUpdate};
use lotse_proto::{AgentKind, PermRequest, SessionId, Status};
use serde_json::{Value, json};
use std::path::{Path, PathBuf};
use support::Screen;

fn fixtures_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../integrations/manifests/fixtures")
}

fn agent_of(name: &str) -> AgentKind {
    serde_json::from_value(json!(name)).unwrap_or_else(|_| panic!("unknown agent {name}"))
}

/// An engine with the screen and OSC 7501 adapters of the bundled manifests (never the user's).
fn engine_for(agent: AgentKind) -> Engine {
    let mut engine = Engine::new(SessionId::new("s1"), agent);
    engine.register(Box::new(Osc7501Adapter::new()));
    let screen = ScreenAdapter::from_set(&ManifestSet::bundled(), agent).expect("bundled manifest");
    engine.register(Box::new(screen));
    engine
}

struct Rig {
    engine: Engine,
    screen: Screen,
    scanner: Osc7501Scanner,
    queries: usize,
}

impl Rig {
    fn new(agent: AgentKind, cols: usize, rows: usize) -> Self {
        Self {
            engine: engine_for(agent),
            screen: Screen::new(cols, rows),
            scanner: Osc7501Scanner::new(),
            queries: 0,
        }
    }

    /// PTY output arrived at `at`: scanned for OSC 7501 and painted; the screen is not sampled yet.
    fn feed(&mut self, text: &str, at: u64) -> Vec<StatusUpdate> {
        let mut updates = Vec::new();
        for osc in self.scanner.feed(text.as_bytes(), at) {
            self.queries += usize::from(osc7501::is_query(&osc.payload));
            updates.extend(self.engine.observe(Observation::Osc(osc)));
        }
        self.screen.feed(text);
        updates
    }

    fn sample(&mut self, at: u64) -> Option<StatusUpdate> {
        self.engine.observe(Observation::Screen(ScreenObservation {
            rows: self.screen.rows(),
            at,
        }))
    }

    fn tick(&mut self, at: u64) -> Option<StatusUpdate> {
        self.engine.observe(Observation::Tick { now: at })
    }
}

// ---- authored fixtures ----------------------------------------------------------------------------

fn run_fixture(name: &str, fixture: &Value) {
    let cols = fixture["cols"].as_u64().unwrap() as usize;
    let rows = fixture["rows"].as_u64().unwrap() as usize;
    let mut rig = Rig::new(agent_of(fixture["agent"].as_str().unwrap()), cols, rows);
    for (i, step) in fixture["steps"].as_array().unwrap().iter().enumerate() {
        let at = step["at"].as_u64().unwrap();
        let ctx = format!("{name} step {i} (t={at})");
        let queries_before = rig.queries;
        let updates: Vec<StatusUpdate> = if let Some(out) = step["out"].as_str() {
            let mut updates = rig.feed(out, at);
            updates.extend(rig.sample(at));
            updates
        } else if step["tick"].as_bool() == Some(true) {
            rig.tick(at).into_iter().collect()
        } else if let Some(p) = step.get("process") {
            let obs = ProcessObservation {
                foreground: p["fg"].as_str().map(str::to_owned),
                exit_code: p["code"].as_i64().map(|c| c as i32),
                at,
            };
            rig.engine
                .observe(Observation::Process(obs))
                .into_iter()
                .collect()
        } else {
            panic!("{ctx}: no out, tick or process");
        };

        assert_eq!(json!(rig.engine.status()), step["expect"], "{ctx}: status");
        if let Some(expected) = step.get("source") {
            let first = updates
                .first()
                .unwrap_or_else(|| panic!("{ctx}: expected an update"));
            assert_eq!(
                &json!(first.source),
                expected,
                "{ctx}: source of the update"
            );
        }
        if let Some(expected) = step.get("perm") {
            let last = updates
                .last()
                .unwrap_or_else(|| panic!("{ctx}: expected an update"));
            assert_eq!(&json!(last.perm), expected, "{ctx}: perm");
        }
        if let Some(expected) = step.get("sub_agents") {
            assert_eq!(
                &json!(rig.engine.sub_agents()),
                expected,
                "{ctx}: sub-agents"
            );
        }
        if let Some(n) = step["queries"].as_u64() {
            assert_eq!(
                (rig.queries - queries_before) as u64,
                n,
                "{ctx}: OSC 7501 queries"
            );
        }
    }
}

fn load_dir(dir: &Path) -> Vec<(String, Value)> {
    let mut files: Vec<_> = std::fs::read_dir(dir)
        .unwrap_or_else(|e| panic!("{}: {e}", dir.display()))
        .map(|e| e.unwrap().path())
        .filter(|p| p.extension().is_some_and(|e| e == "json"))
        .collect();
    files.sort();
    files
        .into_iter()
        .map(|p| {
            let name = p.file_name().unwrap().to_string_lossy().into_owned();
            let value = serde_json::from_str(&std::fs::read_to_string(&p).unwrap())
                .unwrap_or_else(|e| panic!("{name}: {e}"));
            (name, value)
        })
        .collect()
}

#[test]
fn every_authored_fixture_replays_to_its_expected_statuses() {
    let all = load_dir(&fixtures_dir());
    assert!(all.len() >= 5, "fixtures missing: {}", all.len());
    for (name, fixture) in &all {
        assert!(
            fixture["provenance"].is_string(),
            "{name}: say where the transcript comes from"
        );
        run_fixture(name, fixture);
    }
}

#[test]
fn fixtures_cover_codex_opencode_and_osc7501_and_every_status_but_unknown() {
    let mut seen = std::collections::BTreeSet::new();
    let mut agents = std::collections::BTreeSet::new();
    let mut osc = false;
    for (_, fixture) in load_dir(&fixtures_dir()) {
        agents.insert(fixture["agent"].as_str().unwrap().to_owned());
        for step in fixture["steps"].as_array().unwrap() {
            seen.insert(step["expect"].as_str().unwrap().to_owned());
            osc |= step["source"] == "osc7501";
        }
    }
    assert!(
        agents.contains("codex") && agents.contains("opencode") && agents.contains("claude_code"),
        "{agents:?}"
    );
    assert!(osc, "an OSC 7501 transcript");
    for status in ["idle", "run", "perm", "fail", "done"] {
        assert!(seen.contains(status), "no step expects {status}");
    }
}

#[test]
fn the_user_manifest_dir_overrides_the_bundled_manifest_in_a_replay() {
    // A user manifest that calls "Reviewing…" work makes a Codex-like screen Run.
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("codex.toml"),
        "schema = 1\nid = \"codex\"\nagent = \"codex\"\n[[rules]]\nid = \"mine\"\nstate = \"working\"\ncontains = [\"reviewing…\"]\n",
    )
    .unwrap();
    let set = ManifestSet::with_user_dir(dir.path());
    let mut engine = Engine::new(SessionId::new("s"), AgentKind::Codex);
    engine.register(Box::new(
        ScreenAdapter::from_set(&set, AgentKind::Codex).unwrap(),
    ));
    let rows = vec!["Reviewing… the diff".to_owned()];
    let up = engine
        .observe(Observation::Screen(ScreenObservation { rows, at: 1 }))
        .unwrap();
    assert_eq!(up.status, Status::Run);
}

// ---- recorded transcripts -------------------------------------------------------------------------

/// When the daemon samples the screen and ticks: after 100 ms without output, at most every 500 ms
/// while output streams, and a tick every second.
struct DaemonClock {
    dirty: bool,
    last_chunk: u64,
    last_sample: u64,
    next_tick: u64,
}

impl DaemonClock {
    fn new() -> Self {
        Self {
            dirty: false,
            last_chunk: 0,
            last_sample: 0,
            next_tick: 1000,
        }
    }

    /// Fire every sample and tick due before `until`.
    fn advance(&mut self, rig: &mut Rig, until: u64, record: &mut Replayed) {
        loop {
            let sample_due = self
                .dirty
                .then(|| (self.last_chunk + 100).min(self.last_sample + 500));
            let next = sample_due.map_or(self.next_tick, |s| s.min(self.next_tick));
            if next >= until {
                return;
            }
            if sample_due == Some(next) {
                record.add(rig.sample(next));
                self.dirty = false;
                self.last_sample = next;
            } else {
                record.add(rig.tick(next));
                self.next_tick += 1000;
            }
        }
    }
}

/// What a replay reported: the statuses (consecutive duplicates dropped) and the last request.
#[derive(Default)]
struct Replayed {
    statuses: Vec<Status>,
    perm: Option<PermRequest>,
}

impl Replayed {
    fn add(&mut self, update: Option<StatusUpdate>) {
        if let Some(u) = update {
            if u.status == Status::Perm {
                self.perm = u.perm;
            }
            if self.statuses.last() != Some(&u.status) {
                self.statuses.push(u.status);
            }
        }
    }
}

/// Replay `chunks` (ms since spawn, PTY text) and keep sampling for `tail_ms` after the last one.
fn replay_chunks(
    agent: AgentKind,
    cols: usize,
    rows: usize,
    chunks: &[(u64, String)],
    tail_ms: u64,
) -> Replayed {
    let mut rig = Rig::new(agent, cols, rows);
    let mut clock = DaemonClock::new();
    let mut out = Replayed::default();
    let mut last_at = 0;
    for (at, data) in chunks {
        clock.advance(&mut rig, *at, &mut out);
        for u in rig.feed(data, *at) {
            out.add(Some(u));
        }
        (clock.dirty, clock.last_chunk, last_at) = (true, *at, *at);
    }
    clock.advance(&mut rig, last_at + tail_ms, &mut out);
    out
}

#[test]
fn recorded_codex_transcripts_replay_to_the_expected_status_sequence() {
    let all = load_dir(&fixtures_dir().join("recorded"));
    assert!(
        all.len() >= 5,
        "recorded transcripts missing: {}",
        all.len()
    );
    for (name, f) in &all {
        let chunks: Vec<(u64, String)> = f["chunks"]
            .as_array()
            .unwrap()
            .iter()
            .map(|c| (c[0].as_u64().unwrap(), c[1].as_str().unwrap().to_owned()))
            .collect();
        let got = replay_chunks(
            agent_of(f["agent"].as_str().unwrap()),
            f["cols"].as_u64().unwrap() as usize,
            f["rows"].as_u64().unwrap() as usize,
            &chunks,
            f["tail_ms"].as_u64().unwrap(),
        );
        assert_eq!(
            json!(got.statuses),
            f["expect_sequence"],
            "{name}: status sequence"
        );
        if let Some(expected) = f.get("expect_perm") {
            assert_eq!(&json!(got.perm), expected, "{name}: permission request");
        }
    }
}

// ---- exporting Orca's recordings --------------------------------------------------------------------

/// Orca's recorded Codex PTY transcripts (MIT, `src/main/runtime/__fixtures__`): raw bytes as the PTY
/// produced them, at 120x40.
fn orca_dir() -> Option<PathBuf> {
    let dir = std::env::var_os("LOTSE_ORCA_FIXTURES")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            PathBuf::from("/Users/naim/Repos/research/orca/src/main/runtime/__fixtures__")
        });
    dir.is_dir().then_some(dir)
}

/// The recording as (ms, text) chunks: its real timing when it has a `.timing.json`, else one chunk.
fn orca_chunks(dir: &Path, name: &str) -> Vec<(u64, String)> {
    let bytes =
        std::fs::read(dir.join(format!("{name}.txt"))).unwrap_or_else(|e| panic!("{name}: {e}"));
    let text = String::from_utf8_lossy(&bytes).into_owned();
    let Ok(timing) = std::fs::read_to_string(dir.join(format!("{name}.timing.json"))) else {
        return vec![(0, text)];
    };
    let timing: Value = serde_json::from_str(&timing).unwrap();
    let utf16: Vec<u16> = text.encode_utf16().collect();
    let mut offset = 0;
    let chunks = timing["chunks"]
        .as_array()
        .unwrap()
        .iter()
        .map(|c| {
            // Milliseconds since spawn (may be fractional) and the chunk length in UTF-16 units.
            let (at, len) = (
                c[0].as_f64().unwrap() as u64,
                c[1].as_u64().unwrap() as usize,
            );
            offset += len;
            (at, String::from_utf16_lossy(&utf16[offset - len..offset]))
        })
        .collect();
    assert_eq!(
        offset,
        utf16.len(),
        "{name}: timing covers the whole recording"
    );
    chunks
}

/// Regenerates `fixtures/recorded/` from Orca's checkout: `cargo test -p lotse-detect --test
/// transcript_fixtures export_orca_recordings -- --ignored`. The expected sequences are written from
/// what the engine reports now, after they were checked by hand against the recordings' meta notes;
/// review the diff before committing.
#[test]
#[ignore = "reads Orca's checkout and rewrites fixtures/recorded"]
fn export_orca_recordings() {
    let Some(dir) = orca_dir() else {
        panic!("Orca fixtures not found (set LOTSE_ORCA_FIXTURES)");
    };
    // (name, what it shows, tail ms)
    let recordings = [
        (
            "codex-0-158-0-approval",
            "a command approval dialog owns the screen at the end",
            2000,
        ),
        (
            "codex-0-158-0-trustprompt",
            "the trust-this-folder dialog at startup",
            2000,
        ),
        (
            "codex-0158-update-available-dialog",
            "the update-available dialog at startup",
            2000,
        ),
        (
            "codex-0157-plain-ready",
            "a ready composer, nothing happening",
            2000,
        ),
        (
            "codex-0-158-0-timed-turn",
            "one tool-using turn with default animations",
            25_000,
        ),
        (
            "codex-0-155-1-timed-turn",
            "one tool-using turn on 0.155.1, which hides the Working row while it streams an answer",
            25_000,
        ),
    ];
    let out_dir = fixtures_dir().join("recorded");
    std::fs::create_dir_all(&out_dir).unwrap();
    for (name, what, tail_ms) in recordings {
        let chunks = orca_chunks(&dir, name);
        let got = replay_chunks(AgentKind::Codex, 120, 40, &chunks, tail_ms);
        let mut text = format!(
            "{{\n  \"description\": {},\n  \"provenance\": {},\n  \"agent\": \"codex\",\n  \"cols\": 120,\n  \"rows\": 40,\n  \"tail_ms\": {tail_ms},\n  \"expect_sequence\": {},\n",
            json!(what),
            json!(format!(
                "Recorded: raw PTY output of the Codex CLI on macOS, captured by the Orca project (MIT licence; copied from its checkout) as src/main/runtime/__fixtures__/{name}.txt{}; the Codex version is in the Orca recording's .meta.json (and on the first screen). Copied unmodified; chunk times are ms since spawn.",
                if chunks.len() > 1 {
                    " with its .timing.json"
                } else {
                    " (one chunk: no timing was recorded)"
                }
            )),
            json!(got.statuses),
        );
        if let Some(perm) = &got.perm {
            text += &format!("  \"expect_perm\": {},\n", json!(perm));
        }
        text += "  \"chunks\": [\n";
        text += &chunks
            .iter()
            .map(|(at, data)| format!("    {}", json!([at, data])))
            .collect::<Vec<_>>()
            .join(",\n");
        text += "\n  ]\n}\n";
        std::fs::write(out_dir.join(format!("{name}.json")), text).unwrap();
        eprintln!("{name}: {:?} perm={:?}", got.statuses, got.perm);
    }
}
