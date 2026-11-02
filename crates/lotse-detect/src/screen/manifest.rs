//! TOML screen manifests: which text on the terminal screen means which agent state.
//!
//! The format follows herdr's agent-detection manifests (`/Users/naim/Repos/research/herdr/src/detect/manifests/*.toml`,
//! Apache-2.0), simplified: no OSC-title regions (the daemon hands the adapter the visible rows only).
//! Manifests are data, never code, and are never fetched: the bundled ones are compiled in, and a
//! user may drop files into the user manifest directory (`<data dir>/manifests/*.toml`) to replace
//! a bundled manifest by `id` or to add one.
//!
//! ```toml
//! schema = 1                 # required; this loader understands 1
//! id = "codex"               # required; a user file with the same id replaces the bundled one
//! agent = "codex"            # which AgentKind uses it: codex | opencode | claude_code
//! version = "2026.11.02.1"   # free text, for humans
//! hold_ms = 1500             # how long the working signal may be absent before Run becomes Done
//! prompt_markers = ["›"]     # lines starting with one of these are the input prompt (see regions)
//!
//! [[rules]]
//! id = "approval"
//! state = "blocked"          # working | blocked | idle | done | fail | keep (keep: change nothing)
//! priority = 900             # highest priority that matches wins
//! region = "after_last_prompt_marker"
//! tool = "command"           # blocked only: PermRequest.tool (default "approval")
//! summary = '(?m)^\s*\$ (.+?)\s*$'   # blocked only: capture group 1 of the last match on the whole screen becomes PermRequest.summary
//! contains = ["press enter to confirm"]
//! ```
//!
//! **Regions** pick the text a rule looks at: `whole`, `top(N)` / `bottom(N)` (the first / last N
//! non-empty rows), `after_last_prompt_marker` (from the last prompt-marker row down: dialogs and
//! footers), `before_current_prompt_marker` (everything above it: the live status row). Without a
//! marker row, both marker regions are the whole screen.
//!
//! **Matchers** (a rule's own fields, and the entries of `any` / `all` / `not`): `contains` (all
//! substrings present, case-insensitive), `regex` (all match; Rust regex syntax), `any` (at least one
//! sub-matcher matches), `all` (every sub-matcher matches), `not` (no sub-matcher matches). Every
//! field present must hold; a rule needs at least one.

use lotse_proto::AgentKind;
use regex::{Regex, RegexBuilder};
use serde::Deserialize;
use std::path::Path;

/// The manifest schema this loader understands.
pub const SCHEMA: u32 = 1;
const DEFAULT_HOLD_MS: u64 = 1500;
const MAX_NESTING: usize = 8;
const REGEX_SIZE_LIMIT: usize = 1 << 20;
/// A manifest file larger than this is refused (user override files are untrusted input).
const MAX_FILE_BYTES: u64 = 256 * 1024;

#[derive(Debug, thiserror::Error)]
pub enum ManifestError {
    #[error("toml: {0}")]
    Toml(#[from] toml::de::Error),
    #[error("unsupported schema {0} (this build understands {SCHEMA})")]
    Schema(u32),
    #[error("manifest {manifest}: {message}")]
    Invalid { manifest: String, message: String },
    #[error("{path}: {message}")]
    Io { path: String, message: String },
}

fn invalid(manifest: &str, message: impl Into<String>) -> ManifestError {
    ManifestError::Invalid {
        manifest: manifest.to_owned(),
        message: message.into(),
    }
}

// ---- raw (serde) shapes ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawManifest {
    schema: u32,
    id: String,
    agent: Option<AgentKind>,
    #[serde(default)]
    version: String,
    hold_ms: Option<u64>,
    #[serde(default)]
    prompt_markers: Vec<String>,
    #[serde(default)]
    rules: Vec<RawRule>,
}

/// A rule's own fields plus the matcher fields (no `flatten`: serde cannot combine it with
/// `deny_unknown_fields`, and typos in user files should fail loudly).
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawRule {
    id: String,
    state: RuleState,
    #[serde(default)]
    priority: i32,
    #[serde(default = "default_region")]
    region: String,
    tool: Option<String>,
    summary: Option<String>,
    #[serde(default)]
    contains: Vec<String>,
    #[serde(default)]
    regex: Vec<String>,
    #[serde(default)]
    any: Vec<RawMatcher>,
    #[serde(default)]
    all: Vec<RawMatcher>,
    #[serde(default)]
    not: Vec<RawMatcher>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawMatcher {
    #[serde(default)]
    contains: Vec<String>,
    #[serde(default)]
    regex: Vec<String>,
    #[serde(default)]
    any: Vec<RawMatcher>,
    #[serde(default)]
    all: Vec<RawMatcher>,
    #[serde(default)]
    not: Vec<RawMatcher>,
}

fn default_region() -> String {
    "whole".to_owned()
}

// ---- compiled -------------------------------------------------------------------------------------

/// What a matching rule says about the agent.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RuleState {
    Working,
    Blocked,
    Idle,
    Done,
    Fail,
    /// Matches to veto lower rules and leave the state alone (e.g. a transcript viewer).
    Keep,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Region {
    Whole,
    Top(usize),
    Bottom(usize),
    AfterLastPromptMarker,
    BeforeCurrentPromptMarker,
}

impl Region {
    fn parse(s: &str) -> Option<Region> {
        let count = |inner: &str| {
            inner
                .strip_suffix(')')?
                .parse::<usize>()
                .ok()
                .filter(|n| *n > 0)
        };
        match s {
            "whole" => Some(Region::Whole),
            "after_last_prompt_marker" => Some(Region::AfterLastPromptMarker),
            "before_current_prompt_marker" => Some(Region::BeforeCurrentPromptMarker),
            _ => s
                .strip_prefix("top(")
                .and_then(count)
                .map(Region::Top)
                .or_else(|| {
                    s.strip_prefix("bottom(")
                        .and_then(count)
                        .map(Region::Bottom)
                }),
        }
    }
}

#[derive(Debug, Clone, Default)]
struct Matcher {
    /// Lower-cased.
    contains: Vec<String>,
    regex: Vec<Regex>,
    any: Vec<Matcher>,
    all: Vec<Matcher>,
    not: Vec<Matcher>,
}

impl Matcher {
    fn is_empty(&self) -> bool {
        self.contains.is_empty()
            && self.regex.is_empty()
            && self.any.is_empty()
            && self.all.is_empty()
            && self.not.is_empty()
    }

    fn matches(&self, text: &str, lower: &str) -> bool {
        self.contains.iter().all(|s| lower.contains(s.as_str()))
            && self.regex.iter().all(|r| r.is_match(text))
            && (self.any.is_empty() || self.any.iter().any(|m| m.matches(text, lower)))
            && self.all.iter().all(|m| m.matches(text, lower))
            && !self.not.iter().any(|m| m.matches(text, lower))
    }
}

fn compile_regex(manifest: &str, rule: &str, pattern: &str) -> Result<Regex, ManifestError> {
    RegexBuilder::new(pattern)
        .size_limit(REGEX_SIZE_LIMIT)
        .build()
        .map_err(|e| invalid(manifest, format!("rule {rule}: bad regex {pattern:?}: {e}")))
}

fn compile_matcher(
    manifest: &str,
    rule: &str,
    depth: usize,
    contains: &[String],
    regex: &[String],
    groups: [&[RawMatcher]; 3],
) -> Result<Matcher, ManifestError> {
    if depth > MAX_NESTING {
        return Err(invalid(
            manifest,
            format!("rule {rule}: matchers nested deeper than {MAX_NESTING}"),
        ));
    }
    let sub = |list: &[RawMatcher]| {
        list.iter()
            .map(|m| {
                compile_matcher(
                    manifest,
                    rule,
                    depth + 1,
                    &m.contains,
                    &m.regex,
                    [&m.any, &m.all, &m.not],
                )
            })
            .collect::<Result<Vec<_>, _>>()
    };
    Ok(Matcher {
        contains: contains.iter().map(|s| s.to_lowercase()).collect(),
        regex: regex
            .iter()
            .map(|p| compile_regex(manifest, rule, p))
            .collect::<Result<_, _>>()?,
        any: sub(groups[0])?,
        all: sub(groups[1])?,
        not: sub(groups[2])?,
    })
}

#[derive(Debug, Clone)]
pub struct Rule {
    pub id: String,
    pub state: RuleState,
    pub priority: i32,
    region: Region,
    tool: Option<String>,
    summary: Option<Regex>,
    matcher: Matcher,
}

/// A parsed, validated manifest.
#[derive(Debug, Clone)]
pub struct Manifest {
    pub id: String,
    pub agent: Option<AgentKind>,
    pub version: String,
    /// How long (ms) the working signal may be absent before `Run` becomes `Done`.
    pub hold_ms: u64,
    prompt_markers: Vec<String>,
    /// Highest priority first; file order breaks ties.
    rules: Vec<Rule>,
}

/// The first matching rule's verdict.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Verdict {
    pub rule: String,
    pub state: RuleState,
    /// For `Blocked`: what is being asked.
    pub tool: Option<String>,
    pub summary: Option<String>,
}

impl Manifest {
    pub fn parse(text: &str) -> Result<Manifest, ManifestError> {
        let raw: RawManifest = toml::from_str(text)?;
        if raw.schema != SCHEMA {
            return Err(ManifestError::Schema(raw.schema));
        }
        let id = raw.id;
        if id.is_empty()
            || !id
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"-_.".contains(&b))
        {
            return Err(invalid(&id, "id must be [A-Za-z0-9-_.]+"));
        }
        let mut rules = Vec::with_capacity(raw.rules.len());
        for r in raw.rules {
            let region = Region::parse(&r.region).ok_or_else(|| {
                invalid(&id, format!("rule {}: unknown region {:?}", r.id, r.region))
            })?;
            let matcher = compile_matcher(
                &id,
                &r.id,
                0,
                &r.contains,
                &r.regex,
                [&r.any, &r.all, &r.not],
            )?;
            if matcher.is_empty() {
                return Err(invalid(
                    &id,
                    format!("rule {}: no condition (it would match everything)", r.id),
                ));
            }
            if r.state != RuleState::Blocked && (r.tool.is_some() || r.summary.is_some()) {
                return Err(invalid(
                    &id,
                    format!("rule {}: tool/summary only apply to blocked rules", r.id),
                ));
            }
            let summary = r
                .summary
                .as_deref()
                .map(|p| compile_regex(&id, &r.id, p))
                .transpose()?;
            if summary.as_ref().is_some_and(|re| re.captures_len() < 2) {
                return Err(invalid(
                    &id,
                    format!("rule {}: summary needs a capture group", r.id),
                ));
            }
            rules.push(Rule {
                id: r.id,
                state: r.state,
                priority: r.priority,
                region,
                tool: r.tool,
                summary,
                matcher,
            });
        }
        // Stable: equal priorities keep file order.
        rules.sort_by_key(|r| std::cmp::Reverse(r.priority));
        Ok(Manifest {
            id,
            agent: raw.agent,
            version: raw.version,
            hold_ms: raw.hold_ms.unwrap_or(DEFAULT_HOLD_MS),
            prompt_markers: raw.prompt_markers,
            rules,
        })
    }

    pub fn rule_ids(&self) -> impl Iterator<Item = &str> {
        self.rules.iter().map(|r| r.id.as_str())
    }

    /// The verdict of the highest-priority rule that matches `rows`, or `None`.
    pub fn evaluate(&self, rows: &[String]) -> Option<Verdict> {
        let marker = rows.iter().rposition(|row| {
            self.prompt_markers
                .iter()
                .any(|m| row.trim_start().starts_with(m.as_str()))
        });
        for rule in &self.rules {
            let text = region_text(rule.region, rows, marker);
            let lower = text.to_lowercase();
            if !rule.matcher.matches(&text, &lower) {
                continue;
            }
            // The thing being asked sits above the choices, outside the rule's own region, so the
            // summary looks at the whole screen; the last match is the live one.
            let summary = rule
                .summary
                .as_ref()
                .and_then(|re| {
                    re.captures_iter(&rows.join("\n"))
                        .last()
                        .and_then(|c| c.get(1).map(|m| m.as_str().trim().to_owned()))
                })
                .filter(|s| !s.is_empty());
            return Some(Verdict {
                rule: rule.id.clone(),
                state: rule.state,
                tool: rule.tool.clone(),
                summary,
            });
        }
        None
    }
}

fn region_text(region: Region, rows: &[String], marker: Option<usize>) -> String {
    let non_empty = || rows.iter().filter(|r| !r.trim().is_empty());
    let join = |lines: &[&String]| {
        lines
            .iter()
            .map(|s| s.as_str())
            .collect::<Vec<_>>()
            .join("\n")
    };
    match region {
        Region::Whole => rows.join("\n"),
        Region::Top(n) => join(&non_empty().take(n).collect::<Vec<_>>()),
        Region::Bottom(n) => {
            let all: Vec<_> = non_empty().collect();
            join(&all[all.len().saturating_sub(n)..])
        }
        Region::AfterLastPromptMarker => rows[marker.unwrap_or(0)..].join("\n"),
        Region::BeforeCurrentPromptMarker => rows[..marker.unwrap_or(rows.len())].join("\n"),
    }
}

// ---- sets -----------------------------------------------------------------------------------------

const BUNDLED: &[(&str, &str)] = &[
    (
        "codex.toml",
        include_str!("../../../../integrations/manifests/codex.toml"),
    ),
    (
        "opencode.toml",
        include_str!("../../../../integrations/manifests/opencode.toml"),
    ),
];

/// Manifests by id: the bundled ones, with user files layered on top.
#[derive(Debug, Clone, Default)]
pub struct ManifestSet {
    manifests: Vec<Manifest>,
    /// One line per user file that was skipped, for the log and Settings.
    pub warnings: Vec<String>,
}

impl ManifestSet {
    /// The manifests compiled into the binary. A bundled manifest that fails to parse is a build
    /// defect (a test covers it); it is skipped with a warning rather than taking the daemon down.
    pub fn bundled() -> ManifestSet {
        let mut set = ManifestSet::default();
        for (name, text) in BUNDLED {
            match Manifest::parse(text) {
                Ok(m) => set.manifests.push(m),
                Err(e) => set.warnings.push(format!("bundled {name}: {e}")),
            }
        }
        set
    }

    /// Bundled manifests plus every `*.toml` in `user_dir` (a file with a bundled id replaces it; a
    /// missing directory is fine; a broken file is skipped and reported in `warnings`).
    pub fn with_user_dir(user_dir: &Path) -> ManifestSet {
        let mut set = ManifestSet::bundled();
        let Ok(entries) = std::fs::read_dir(user_dir) else {
            return set;
        };
        let mut files: Vec<_> = entries
            .filter_map(Result::ok)
            .map(|e| e.path())
            .filter(|p| p.extension().is_some_and(|e| e == "toml"))
            .collect();
        files.sort();
        for path in files {
            match load_file(&path) {
                Ok(m) => set.insert(m),
                Err(e) => set.warnings.push(e.to_string()),
            }
        }
        for warning in &set.warnings {
            tracing::warn!(target: "lotse_detect::screen", "{warning}");
        }
        set
    }

    fn insert(&mut self, manifest: Manifest) {
        match self.manifests.iter_mut().find(|m| m.id == manifest.id) {
            Some(slot) => {
                if slot.agent != manifest.agent {
                    // The agent would silently lose its screen detection.
                    self.warnings.push(format!(
                        "manifest {}: replaces the bundled one but declares agent {:?} instead of {:?}",
                        manifest.id, manifest.agent, slot.agent
                    ));
                }
                *slot = manifest;
            }
            None => self.manifests.push(manifest),
        }
    }

    pub fn get(&self, id: &str) -> Option<&Manifest> {
        self.manifests.iter().find(|m| m.id == id)
    }

    /// The manifest for `agent` (a user manifest wins over a bundled one by replacing the same id; two
    /// ids claiming one agent: the later one, i.e. the user's).
    pub fn for_agent(&self, agent: AgentKind) -> Option<&Manifest> {
        self.manifests.iter().rev().find(|m| m.agent == Some(agent))
    }

    pub fn ids(&self) -> impl Iterator<Item = &str> {
        self.manifests.iter().map(|m| m.id.as_str())
    }
}

fn load_file(path: &Path) -> Result<Manifest, ManifestError> {
    let io = |message: String| ManifestError::Io {
        path: path.display().to_string(),
        message,
    };
    let meta = std::fs::metadata(path).map_err(|e| io(e.to_string()))?;
    if !meta.is_file() || meta.len() > MAX_FILE_BYTES {
        return Err(io(format!(
            "not a regular file under {MAX_FILE_BYTES} bytes"
        )));
    }
    let text = std::fs::read_to_string(path).map_err(|e| io(e.to_string()))?;
    Manifest::parse(&text).map_err(|e| io(e.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rows(lines: &[&str]) -> Vec<String> {
        lines.iter().map(|s| (*s).to_owned()).collect()
    }

    const SAMPLE: &str = r#"
        schema = 1
        id = "t"
        agent = "codex"
        prompt_markers = ["›"]

        [[rules]]
        id = "low"
        state = "idle"
        priority = 1
        contains = ["ready"]

        [[rules]]
        id = "high"
        state = "blocked"
        priority = 9
        region = "after_last_prompt_marker"
        tool = "command"
        summary = '(?m)^\s*\$ (.+?)\s*$'
        contains = ["confirm"]
        not = [{ contains = ["never"] }]
        any = [{ contains = ["enter"] }, { regex = ['esc\s+cancel'] }]
    "#;

    #[test]
    fn highest_priority_matching_rule_wins_and_extracts_the_summary() {
        let m = Manifest::parse(SAMPLE).unwrap();
        let screen = rows(&[
            "ready",
            "› 1. Yes",
            "  $ rm -rf build",
            "Press ENTER to confirm",
        ]);
        let v = m.evaluate(&screen).unwrap();
        assert_eq!((v.rule.as_str(), v.state), ("high", RuleState::Blocked));
        assert_eq!(v.tool.as_deref(), Some("command"));
        assert_eq!(v.summary.as_deref(), Some("rm -rf build"));
        // `not` vetoes, so the low rule answers.
        let veto = rows(&["ready", "› 1. Yes", "never confirm enter"]);
        assert_eq!(m.evaluate(&veto).unwrap().rule, "low");
        assert_eq!(m.evaluate(&rows(&["nothing here"])), None);
    }

    #[test]
    fn regions_select_the_text() {
        let m = Manifest::parse(
            r#"
            schema = 1
            id = "r"
            prompt_markers = [">"]
            [[rules]]
            id = "above"
            state = "working"
            region = "before_current_prompt_marker"
            contains = ["busy"]
            [[rules]]
            id = "top"
            state = "keep"
            priority = -1
            region = "top(1)"
            contains = ["header"]
            [[rules]]
            id = "bottom"
            state = "done"
            priority = -2
            region = "bottom(1)"
            contains = ["footer"]
            "#,
        )
        .unwrap();
        let s = |lines: &[&str]| m.evaluate(&rows(lines)).map(|v| v.rule);
        assert_eq!(s(&["busy", "> input", "footer"]).as_deref(), Some("above"));
        assert_eq!(
            s(&["> input", "busy"]).as_deref(),
            None,
            "below the prompt is not above it"
        );
        assert_eq!(
            s(&["", "header", "x"]).as_deref(),
            Some("top"),
            "empty rows do not count"
        );
        assert_eq!(s(&["x", "y", "footer"]).as_deref(), Some("bottom"));
        assert_eq!(s(&["header", "footer", "x"]).as_deref(), Some("top"));
        // No prompt marker: before/after regions are the whole screen.
        assert_eq!(s(&["busy"]).as_deref(), Some("above"));
    }

    #[test]
    fn rejects_bad_manifests() {
        let bad = |extra: &str| {
            Manifest::parse(&format!("schema = 1\nid = \"x\"\n{extra}"))
                .unwrap_err()
                .to_string()
        };
        assert!(
            Manifest::parse("schema = 2\nid = \"x\"")
                .unwrap_err()
                .to_string()
                .contains("schema 2")
        );
        assert!(bad("[[rules]]\nid=\"a\"\nstate=\"idle\"").contains("no condition"));
        assert!(bad("[[rules]]\nid=\"a\"\nstate=\"idle\"\nregex=[\"(\"]").contains("bad regex"));
        assert!(
            bad("[[rules]]\nid=\"a\"\nstate=\"idle\"\nregion=\"nope\"\ncontains=[\"x\"]")
                .contains("unknown region")
        );
        assert!(
            bad("[[rules]]\nid=\"a\"\nstate=\"idle\"\nregion=\"top(0)\"\ncontains=[\"x\"]")
                .contains("unknown region")
        );
        assert!(
            bad("[[rules]]\nid=\"a\"\nstate=\"idle\"\ntool=\"t\"\ncontains=[\"x\"]")
                .contains("only apply to blocked")
        );
        assert!(
            bad("[[rules]]\nid=\"a\"\nstate=\"blocked\"\nsummary=\"no group\"\ncontains=[\"x\"]")
                .contains("capture group")
        );
        assert!(
            bad("typo = 1").contains("toml"),
            "unknown top-level keys fail"
        );
        assert!(bad("[[rules]]\nid=\"a\"\nstate=\"idle\"\ncontain=[\"x\"]").contains("toml"));
    }

    #[test]
    fn deeply_nested_matchers_are_refused() {
        let mut nested = String::from("{ contains = [\"x\"] }");
        for _ in 0..=MAX_NESTING {
            nested = format!("{{ any = [{nested}] }}");
        }
        let text = format!(
            "schema = 1\nid = \"x\"\n[[rules]]\nid = \"a\"\nstate = \"idle\"\nany = [{nested}]"
        );
        assert!(
            Manifest::parse(&text)
                .unwrap_err()
                .to_string()
                .contains("nested")
        );
    }

    #[test]
    fn bundled_manifests_all_parse() {
        let set = ManifestSet::bundled();
        assert!(set.warnings.is_empty(), "{:?}", set.warnings);
        assert_eq!(
            set.for_agent(AgentKind::Codex).map(|m| m.id.as_str()),
            Some("codex")
        );
        assert_eq!(
            set.for_agent(AgentKind::OpenCode).map(|m| m.id.as_str()),
            Some("opencode")
        );
        assert!(set.for_agent(AgentKind::Terminal).is_none());
    }

    #[test]
    fn a_user_manifest_that_drops_the_agent_of_a_bundled_one_is_reported() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("codex.toml"),
            "schema = 1\nid = \"codex\"\n",
        )
        .unwrap();
        let set = ManifestSet::with_user_dir(dir.path());
        assert_eq!(set.warnings.len(), 1, "{:?}", set.warnings);
        assert!(set.warnings[0].contains("declares agent"));
        assert!(set.for_agent(AgentKind::Codex).is_none());
    }

    #[test]
    fn user_dir_overrides_adds_and_reports_broken_files() {
        let dir = tempfile::tempdir().unwrap();
        // Same id as a bundled one: replaces it.
        std::fs::write(
            dir.path().join("codex.toml"),
            "schema = 1\nid = \"codex\"\nagent = \"codex\"\nversion = \"user\"\n[[rules]]\nid = \"a\"\nstate = \"working\"\ncontains = [\"zzz\"]\n",
        )
        .unwrap();
        // New id.
        std::fs::write(
            dir.path().join("mine.toml"),
            "schema = 1\nid = \"mine\"\nagent = \"claude_code\"\n",
        )
        .unwrap();
        std::fs::write(dir.path().join("broken.toml"), "schema = 1\nid = ").unwrap();
        std::fs::write(dir.path().join("ignored.txt"), "x").unwrap();

        let set = ManifestSet::with_user_dir(dir.path());
        assert_eq!(set.get("codex").unwrap().version, "user");
        assert_eq!(
            set.get("codex").unwrap().rule_ids().collect::<Vec<_>>(),
            ["a"]
        );
        assert!(set.get("mine").is_some());
        assert!(
            set.get("opencode").is_some(),
            "other bundled manifests stay"
        );
        assert_eq!(set.warnings.len(), 1);
        assert!(set.warnings[0].contains("broken.toml"));

        // A missing directory is fine.
        let missing = ManifestSet::with_user_dir(&dir.path().join("nope"));
        assert!(missing.warnings.is_empty());
    }
}
