//! Screen-scrape adapter driven by TOML manifests (herdr style), plus process identity.
//! Sources: [`DetectSource::Screen`] and [`DetectSource::Process`]. No remote manifest fetch, ever.
//!
//! # What the daemon feeds
//!
//! - [`Observation::Screen`]: the visible rows of the session's VT model (`alacritty_terminal`), once
//!   PTY output has been quiet for about 100 ms (and at most every ~500 ms while it streams), rows
//!   top to bottom with trailing blanks trimmed.
//! - [`Observation::Tick`]: about once a second, so "the working signal has been gone for
//!   `hold_ms`" fires even when the screen stops changing.
//! - [`Observation::Process`]: the PTY's foreground process group changed (`tcgetpgrp` + process
//!   name), or the PTY child exited. [`identify_agent`] tells which agent a process name is.
//!
//! # Status table
//!
//! The manifest ([`manifest`]) names the screen state; the adapter turns it into a status:
//!
//! | Screen verdict | Effect |
//! |---|---|
//! | `working` | `Run` at once |
//! | `blocked` | `Perm` at once, with `PermRequest { tool, summary }` from the rule |
//! | `done` / `fail` | `Done` / `Fail` at once |
//! | `keep` rule | nothing changes |
//! | `idle` rule, or a non-empty screen no rule matches ("at rest") | after `Run` or `Perm`: once the rest has lasted `hold_ms` (a flicker between tool calls must not end the turn) `Done`, or `Idle` if the agent never worked (a startup dialog); after `Done` / `Fail`: nothing (the unseen result stays until the next `working`); otherwise `Idle` |
//! | empty screen | nothing changes |
//!
//! Process observations (source `Process`, the weakest):
//!
//! | Observation | Effect |
//! |---|---|
//! | PTY child exited, code 0, 130 or 143 | screen state cleared; `Idle` |
//! | PTY child exited, any other code | screen state cleared; `Fail` |
//! | a shell took the foreground after the agent was seen | screen state cleared; `Idle` (the agent quit) |
//! | the agent in the foreground | no signal; remembers the agent is running |
//!
//! Only a *shell* counts as "the agent quit": agents run tools as child processes, and a tool in the
//! foreground must not end the session. Exit codes 130 / 143 (SIGINT / SIGTERM) are the user quitting,
//! not a failure (a heuristic).

pub mod manifest;

use crate::adapter::{Adapter, Detection, Observation, ProcessObservation, ScreenObservation};
use lotse_proto::{AgentKind, DetectSource, PermRequest, SessionId, Status, UnixMs};
use manifest::{Manifest, ManifestSet, RuleState, Verdict};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::SystemTime;

/// Longest `PermRequest::summary`, in characters.
const SUMMARY_MAX_CHARS: usize = 160;
const DEFAULT_PERM_TOOL: &str = "approval";
const DEFAULT_PERM_SUMMARY: &str = "Waiting for your input";

// ---- process identity ---------------------------------------------------------------------------

fn basename(name: &str) -> &str {
    let name = name.rsplit('/').next().unwrap_or(name);
    name.strip_prefix('-').unwrap_or(name)
}

/// The agent a foreground process name is (`claude`, `codex`, `opencode`; a path, a leading `-` and
/// case are ignored), or `None`. Agents that run under a generic interpreter (`node`) show up under
/// their script name only if the daemon passes that; otherwise this answers `None`.
pub fn identify_agent(process: &str) -> Option<AgentKind> {
    match basename(process).to_ascii_lowercase().as_str() {
        "claude" | "claude-code" => Some(AgentKind::ClaudeCode),
        "codex" => Some(AgentKind::Codex),
        "opencode" | ".opencode" => Some(AgentKind::OpenCode),
        _ => None,
    }
}

/// Whether a process name is an interactive shell (the agent is no longer in the foreground).
pub fn is_shell(process: &str) -> bool {
    matches!(
        basename(process).to_ascii_lowercase().as_str(),
        "sh" | "bash"
            | "zsh"
            | "fish"
            | "dash"
            | "ksh"
            | "csh"
            | "tcsh"
            | "nu"
            | "elvish"
            | "xonsh"
            | "login"
    )
}

// ---- manifest lookup ----------------------------------------------------------------------------

/// Directory holding user manifest overrides: `<data dir>/manifests`, where the data dir is
/// `$LOTSE_DATA_DIR`, else `~/Library/Application Support/Lotse` (the same rule as `lotse-hook`).
pub fn user_manifest_dir() -> Option<PathBuf> {
    let data = match std::env::var_os("LOTSE_DATA_DIR").filter(|v| !v.is_empty()) {
        Some(dir) => PathBuf::from(dir),
        None => PathBuf::from(std::env::var_os("HOME").filter(|v| !v.is_empty())?)
            .join("Library/Application Support/Lotse"),
    };
    Some(data.join("manifests"))
}

/// Called by `Engine::with_default_adapters`: the manifest-driven adapter for `agent`, using the
/// bundled manifests and the user override directory ([`user_manifest_dir`]). The parsed set is
/// cached and reloaded only when the user directory changed.
pub fn default_adapter(_session: &SessionId, agent: AgentKind) -> Option<Box<dyn Adapter>> {
    static CACHE: SetCache = SetCache(Mutex::new(None));
    let set = CACHE.get(user_manifest_dir().as_deref());
    ScreenAdapter::from_set(&set, agent).map(|a| Box::new(a) as Box<dyn Adapter>)
}

/// What a manifest directory looked like when its set was parsed: the directory's own mtime plus
/// name, mtime and size of every `*.toml` (an in-place edit does not touch the directory's mtime).
type DirStamp = Vec<(Option<std::ffi::OsString>, Option<SystemTime>, u64)>;

fn dir_stamp(dir: &Path) -> DirStamp {
    let stat = |p: &Path| std::fs::metadata(p).ok();
    let mut stamp = vec![(
        None,
        stat(dir).and_then(|m| m.modified().ok()),
        0, // directory size is meaningless
    )];
    let mut files: Vec<_> = std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|e| e == "toml"))
        .collect();
    files.sort();
    for path in files {
        let meta = stat(&path);
        stamp.push((
            path.file_name().map(ToOwned::to_owned),
            meta.as_ref().and_then(|m| m.modified().ok()),
            meta.map_or(0, |m| m.len()),
        ));
    }
    stamp
}

struct Cached {
    dir: Option<PathBuf>,
    stamp: DirStamp,
    set: Arc<ManifestSet>,
}

struct SetCache(Mutex<Option<Cached>>);

impl SetCache {
    fn get(&self, dir: Option<&Path>) -> Arc<ManifestSet> {
        let stamp = dir.map(dir_stamp).unwrap_or_default();
        let mut slot = self.0.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(c) = slot.as_ref()
            && c.dir.as_deref() == dir
            && c.stamp == stamp
        {
            return c.set.clone();
        }
        let set = Arc::new(match dir {
            Some(dir) => ManifestSet::with_user_dir(dir),
            None => ManifestSet::bundled(),
        });
        *slot = Some(Cached {
            dir: dir.map(Path::to_path_buf),
            stamp,
            set: set.clone(),
        });
        set
    }
}

// ---- adapter ------------------------------------------------------------------------------------

/// Screen and process adapter for one session.
#[derive(Debug)]
pub struct ScreenAdapter {
    agent: AgentKind,
    manifest: Arc<Manifest>,
    /// What the screen said last (`Unknown`: no opinion).
    status: Status,
    perm: Option<PermRequest>,
    /// Since when the screen has shown "at rest" while the status is `Run` / `Perm`.
    rest_since: Option<UnixMs>,
    /// The agent was seen working since the screen state was last cleared.
    worked: bool,
    /// What the process source said last.
    process: Status,
    agent_seen: bool,
}

impl ScreenAdapter {
    pub fn new(agent: AgentKind, manifest: Arc<Manifest>) -> Self {
        Self {
            agent,
            manifest,
            status: Status::Unknown,
            perm: None,
            rest_since: None,
            worked: false,
            process: Status::Unknown,
            agent_seen: false,
        }
    }

    /// The adapter for `agent` from `set`, or `None` when no manifest claims the agent.
    pub fn from_set(set: &ManifestSet, agent: AgentKind) -> Option<Self> {
        set.for_agent(agent)
            .map(|m| Self::new(agent, Arc::new(m.clone())))
    }

    fn set(&mut self, status: Status, perm: Option<PermRequest>, at: UnixMs) -> Vec<Detection> {
        self.rest_since = None;
        self.worked |= status == Status::Run;
        if self.status == status && self.perm == perm {
            return Vec::new();
        }
        self.status = status;
        self.perm = perm.clone();
        vec![self.detection(DetectSource::Screen, status, perm, at)]
    }

    fn detection(
        &self,
        source: DetectSource,
        status: Status,
        perm: Option<PermRequest>,
        at: UnixMs,
    ) -> Detection {
        Detection {
            source,
            status: Some(status),
            sub_agents: None,
            perm,
            at,
        }
    }

    fn on_screen(&mut self, screen: &ScreenObservation) -> Vec<Detection> {
        let at = screen.at;
        let verdict = self.manifest.evaluate(&screen.rows);
        let state = match &verdict {
            Some(v) => v.state,
            None if screen.rows.iter().all(|r| r.trim().is_empty()) => return Vec::new(),
            None => RuleState::Idle,
        };
        match state {
            RuleState::Keep => Vec::new(),
            RuleState::Working => self.set(Status::Run, None, at),
            RuleState::Blocked => {
                let perm = perm_request(verdict.as_ref());
                self.set(Status::Perm, Some(perm), at)
            }
            RuleState::Done => self.set(Status::Done, None, at),
            RuleState::Fail => self.set(Status::Fail, None, at),
            RuleState::Idle => match self.status {
                Status::Run | Status::Perm => {
                    self.rest_since.get_or_insert(at);
                    self.release(at)
                }
                Status::Done | Status::Fail => {
                    self.rest_since = None;
                    Vec::new()
                }
                _ => self.set(Status::Idle, None, at),
            },
        }
    }

    /// `Run` / `Perm` ends once the screen has been at rest for `hold_ms`: `Done` after real work,
    /// `Idle` when the agent only showed a startup dialog.
    fn release(&mut self, now: UnixMs) -> Vec<Detection> {
        match self.rest_since {
            Some(since) if now.saturating_sub(since) >= self.manifest.hold_ms => {
                let status = if self.worked {
                    Status::Done
                } else {
                    Status::Idle
                };
                self.set(status, None, now)
            }
            _ => Vec::new(),
        }
    }

    fn clear_screen(&mut self, at: UnixMs) -> Option<Detection> {
        self.rest_since = None;
        self.perm = None;
        self.worked = false;
        (self.status != Status::Unknown).then(|| {
            self.status = Status::Unknown;
            self.detection(DetectSource::Screen, Status::Unknown, None, at)
        })
    }

    fn set_process(&mut self, status: Status, at: UnixMs) -> Option<Detection> {
        (self.process != status).then(|| {
            self.process = status;
            self.detection(DetectSource::Process, status, None, at)
        })
    }

    fn on_process(&mut self, p: &ProcessObservation) -> Vec<Detection> {
        let mut out = Vec::new();
        match p.foreground.as_deref() {
            None => {
                out.extend(self.clear_screen(p.at));
                let failed = p.exit_code.is_some_and(|c| !matches!(c, 0 | 130 | 143));
                out.extend(
                    self.set_process(if failed { Status::Fail } else { Status::Idle }, p.at),
                );
                self.agent_seen = false;
            }
            Some(name) if identify_agent(name) == Some(self.agent) => {
                self.agent_seen = true;
                out.extend(self.set_process(Status::Unknown, p.at));
            }
            Some(name) if self.agent_seen && is_shell(name) => {
                out.extend(self.clear_screen(p.at));
                out.extend(self.set_process(Status::Idle, p.at));
                self.agent_seen = false;
            }
            Some(_) => {}
        }
        out
    }
}

fn perm_request(verdict: Option<&Verdict>) -> PermRequest {
    let tool = verdict
        .and_then(|v| v.tool.as_deref())
        .unwrap_or(DEFAULT_PERM_TOOL)
        .to_owned();
    let summary = verdict
        .and_then(|v| v.summary.as_deref())
        .unwrap_or(DEFAULT_PERM_SUMMARY);
    PermRequest {
        tool,
        summary: summary.chars().take(SUMMARY_MAX_CHARS).collect(),
    }
}

impl Adapter for ScreenAdapter {
    fn name(&self) -> &'static str {
        "screen"
    }

    fn agent(&self) -> AgentKind {
        self.agent
    }

    fn source(&self) -> DetectSource {
        DetectSource::Screen
    }

    fn observe(&mut self, obs: &Observation) -> Vec<Detection> {
        match obs {
            Observation::Screen(screen) => self.on_screen(screen),
            Observation::Tick { now } => self.release(*now),
            Observation::Process(p) => self.on_process(p),
            Observation::Hook(_) | Observation::Osc(_) => Vec::new(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const MANIFEST: &str = r#"
        schema = 1
        id = "t"
        agent = "codex"
        hold_ms = 1000
        prompt_markers = [">"]
        [[rules]]
        id = "busy"
        state = "working"
        priority = 5
        contains = ["working"]
        [[rules]]
        id = "ask"
        state = "blocked"
        priority = 9
        tool = "command"
        summary = '(?m)^\$ (.+)$'
        contains = ["allow?"]
        [[rules]]
        id = "viewer"
        state = "keep"
        priority = 20
        contains = ["pager"]
    "#;

    fn adapter() -> ScreenAdapter {
        ScreenAdapter::new(
            AgentKind::Codex,
            Arc::new(Manifest::parse(MANIFEST).unwrap()),
        )
    }

    fn screen(a: &mut ScreenAdapter, at: UnixMs, rows: &[&str]) -> Vec<(DetectSource, Status)> {
        a.observe(&Observation::Screen(ScreenObservation {
            rows: rows.iter().map(|s| (*s).to_owned()).collect(),
            at,
        }))
        .into_iter()
        .map(|d| (d.source, d.status.unwrap()))
        .collect()
    }

    fn tick(a: &mut ScreenAdapter, at: UnixMs) -> Vec<Status> {
        a.observe(&Observation::Tick { now: at })
            .into_iter()
            .map(|d| d.status.unwrap())
            .collect()
    }

    fn scr(status: Status) -> Vec<(DetectSource, Status)> {
        vec![(DetectSource::Screen, status)]
    }

    #[test]
    fn manifest_set_is_cached_until_the_user_dir_changes() {
        let dir = tempfile::tempdir().unwrap();
        let cache = SetCache(Mutex::new(None));
        let first = cache.get(Some(dir.path()));
        assert!(Arc::ptr_eq(&first, &cache.get(Some(dir.path()))));

        std::fs::write(
            dir.path().join("mine.toml"),
            "schema = 1\nid = \"mine\"\nagent = \"claude_code\"\n",
        )
        .unwrap();
        let second = cache.get(Some(dir.path()));
        assert!(!Arc::ptr_eq(&first, &second));
        assert!(second.get("mine").is_some());
        // an in-place edit of the same file (new size) reloads too
        std::fs::write(
            dir.path().join("mine.toml"),
            "schema = 1\nid = \"mine\"\nagent = \"claude_code\"\nversion = \"2\"\n",
        )
        .unwrap();
        assert_eq!(
            cache.get(Some(dir.path())).get("mine").unwrap().version,
            "2"
        );
        // another directory is another set
        assert!(cache.get(None).get("mine").is_none());
    }

    #[test]
    fn working_then_rest_becomes_done_after_the_hold_and_stays() {
        let mut a = adapter();
        assert!(
            screen(&mut a, 0, &[]).is_empty(),
            "empty screen: no opinion"
        );
        assert_eq!(screen(&mut a, 1, &["> hi"]), scr(Status::Idle));
        assert_eq!(screen(&mut a, 2, &["working"]), scr(Status::Run));
        assert!(screen(&mut a, 3, &["working 2"]).is_empty());
        // Rest begins at 100; not yet 1000 ms.
        assert!(screen(&mut a, 100, &["> hi"]).is_empty());
        assert!(tick(&mut a, 900).is_empty());
        // A flicker back to working resets the hold.
        assert_eq!(screen(&mut a, 950, &["working"]), Vec::new());
        assert!(screen(&mut a, 1000, &["> hi"]).is_empty());
        assert!(tick(&mut a, 1999).is_empty());
        assert_eq!(tick(&mut a, 2000), [Status::Done]);
        // Done stays through rest, and a new turn starts a new Run.
        assert!(screen(&mut a, 3000, &["> hi"]).is_empty());
        assert!(tick(&mut a, 9000).is_empty());
        assert_eq!(screen(&mut a, 9500, &["working"]), scr(Status::Run));
    }

    #[test]
    fn screens_alone_can_release_without_ticks() {
        let mut a = adapter();
        screen(&mut a, 0, &["working"]);
        assert!(screen(&mut a, 10, &["> idle"]).is_empty());
        assert_eq!(screen(&mut a, 1010, &["> idle"]), scr(Status::Done));
    }

    #[test]
    fn blocked_carries_the_request_and_a_keep_rule_changes_nothing() {
        let mut a = adapter();
        screen(&mut a, 0, &["working"]);
        let det = a.observe(&Observation::Screen(ScreenObservation {
            rows: vec!["allow?".into(), "$ rm -rf build".into()],
            at: 5,
        }));
        assert_eq!(det[0].status, Some(Status::Perm));
        assert_eq!(
            det[0].perm,
            Some(PermRequest {
                tool: "command".into(),
                summary: "rm -rf build".into()
            })
        );
        // A pager (keep) with a blocked-looking screen underneath changes nothing.
        assert!(screen(&mut a, 6, &["pager", "allow?", "$ other"]).is_empty());
        // Dialog gone: still Perm for the hold, then Done.
        assert!(screen(&mut a, 7, &["> idle"]).is_empty());
        assert_eq!(tick(&mut a, 1007), [Status::Done]);
    }

    #[test]
    fn blocked_without_a_summary_uses_the_defaults() {
        let mut a = adapter();
        let det = a.observe(&Observation::Screen(ScreenObservation {
            rows: vec!["allow?".into()],
            at: 1,
        }));
        assert_eq!(
            det[0].perm,
            Some(PermRequest {
                tool: "command".into(),
                summary: DEFAULT_PERM_SUMMARY.into()
            })
        );
    }

    fn proc(
        a: &mut ScreenAdapter,
        fg: Option<&str>,
        code: Option<i32>,
        at: UnixMs,
    ) -> Vec<(DetectSource, Status)> {
        a.observe(&Observation::Process(ProcessObservation {
            foreground: fg.map(Into::into),
            exit_code: code,
            at,
        }))
        .into_iter()
        .map(|d| (d.source, d.status.unwrap()))
        .collect()
    }

    #[test]
    fn process_events_clear_the_screen_state() {
        let mut a = adapter();
        assert!(proc(&mut a, Some("/opt/homebrew/bin/codex"), None, 1).is_empty());
        screen(&mut a, 2, &["working"]);
        // A tool in the foreground is not the agent quitting.
        assert!(proc(&mut a, Some("git"), None, 3).is_empty());
        assert_eq!(
            proc(&mut a, Some("-zsh"), None, 4),
            [
                (DetectSource::Screen, Status::Unknown),
                (DetectSource::Process, Status::Idle)
            ]
        );
        // Child exit: nonzero is a failure, SIGINT is not.
        screen(&mut a, 5, &["working"]);
        assert_eq!(
            proc(&mut a, None, Some(1), 6),
            [
                (DetectSource::Screen, Status::Unknown),
                (DetectSource::Process, Status::Fail)
            ]
        );
        let mut b = adapter();
        assert_eq!(
            proc(&mut b, None, Some(130), 1),
            [(DetectSource::Process, Status::Idle)]
        );
    }

    #[test]
    fn a_shell_in_the_foreground_before_the_agent_was_seen_is_ignored() {
        let mut a = adapter();
        assert!(
            proc(&mut a, Some("zsh"), None, 1).is_empty(),
            "the login shell that starts the agent"
        );
    }

    #[test]
    fn identifies_agents_and_shells_by_process_name() {
        assert_eq!(identify_agent("claude"), Some(AgentKind::ClaudeCode));
        assert_eq!(
            identify_agent("/opt/homebrew/bin/Codex"),
            Some(AgentKind::Codex)
        );
        assert_eq!(identify_agent("-opencode"), Some(AgentKind::OpenCode));
        assert_eq!(identify_agent("node"), None);
        assert_eq!(identify_agent("codex-helper"), None);
        assert!(
            is_shell("/bin/zsh")
                && is_shell("-bash")
                && !is_shell("zshrc-tool")
                && !is_shell("codex")
        );
    }

    #[test]
    fn from_set_picks_the_manifest_of_the_agent() {
        let set = ManifestSet::bundled();
        assert!(ScreenAdapter::from_set(&set, AgentKind::Codex).is_some());
        assert!(ScreenAdapter::from_set(&set, AgentKind::OpenCode).is_some());
        assert!(ScreenAdapter::from_set(&set, AgentKind::Terminal).is_none());
    }
}
