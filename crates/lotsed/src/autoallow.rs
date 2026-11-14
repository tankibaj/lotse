//! Auto-allow rules (Beta feature 14, **off by default**): the daemon decides, per tool call, whether a
//! permission prompt may be skipped. SECURITY-CRITICAL, so it errs towards "ask".
//!
//! - **Deny by default.** No rules, a disabled config, an unreadable file, an unknown tool, a rule of
//!   another project, any doubt: no match.
//! - **Exact and anchored.** No regex, no glob, no substring. A Bash rule is an exact command or a
//!   word-anchored prefix; a path rule is a directory of the project. Case-sensitive.
//! - **Plain commands only.** A command with anything but `[A-Za-z0-9 _./:=@+,-]` (one space between
//!   words) never matches, whatever the rule says: no `; & | < > $ ` ( ) { } * ? ! ~ ' " \`, no newline,
//!   no tab, no non-ASCII. A chained command is therefore never auto-allowed.
//! - **Destructive and out-of-project is explicit.** `rm`, `sudo`, `curl`, `git push`, … never match
//!   unless the rule says `allow_destructive`; a path outside the session folder, a `..` anywhere, a
//!   symlink that leads out, and secrets folders and files (`.git`, `.env*`, `.ssh`, keys) likewise.
//! - **Per project.** A rule applies to the sessions of its project only.
//! - **Daemon only.** The rules live in `<data dir>/autoallow.json` (0600, read back only when it is
//!   ours and private); the app, the hook and the mod never hold them. `lotse-hook` and the mod *ask*
//!   (`HookEvent` with `kind: "lotse_autoallow"`) and allow only on an `ok` answer, which the daemon
//!   gives after it wrote the decision to history (a failed write is a "no").
//!
//! Setting the rules from the app needs a request the frozen proto does not have yet: see
//! `AutoAllow::set_config` (the app's `Requests` entry in the work package report).

use crate::db::Db;
use crate::session::now_ms;
use crate::state::State;
use lotse_proto::{ErrorCode, ProjectId, ProtoError, Response, SessionId, UnixMs};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::io::Write;
use std::os::unix::fs::MetadataExt;
use std::path::{Component, Path, PathBuf};
use std::sync::RwLock;
use std::sync::atomic::{AtomicU64, Ordering};

/// File of the rules inside the data dir.
pub const FILE: &str = "autoallow.json";
/// `payload.kind` of the question `lotse-hook` and the mod put to the daemon.
pub const QUERY_KIND: &str = "lotse_autoallow";
/// Who may ask: hello `client` of `lotse-hook` (PreToolUse) and of `lotse-hook mod-allow`.
const HOOK_CLIENT: &str = "hook";

const MAX_RULES: usize = 200;
const MAX_FIELD: usize = 1000;
const MAX_FILE_BYTES: u64 = 256 * 1024;
const MAX_SUMMARY_CHARS: usize = 200;

/// Tools whose rule is a directory, with the input key that names the path.
const PATH_TOOLS: [(&str, &str); 7] = [
    ("Read", "file_path"),
    ("Edit", "file_path"),
    ("MultiEdit", "file_path"),
    ("Write", "file_path"),
    ("NotebookEdit", "notebook_path"),
    ("Glob", "path"),
    ("Grep", "path"),
];

/// Commands that change or leave the machine. A rule can name one only with `allow_destructive`.
const DESTRUCTIVE_WORDS: &[&str] = &[
    "rm",
    "rmdir",
    "mv",
    "dd",
    "mkfs",
    "sudo",
    "su",
    "doas",
    "chmod",
    "chown",
    "chgrp",
    "kill",
    "killall",
    "pkill",
    "shutdown",
    "reboot",
    "curl",
    "wget",
    "ssh",
    "scp",
    "rsync",
    "nc",
    "eval",
    "exec",
    "sh",
    "bash",
    "zsh",
    "xargs",
    "tee",
    "truncate",
    "shred",
    "ln",
    "open",
    "osascript",
    "launchctl",
    "defaults",
    "docker",
    "kubectl",
    "publish",
    "unpublish",
    "dlx",
];
/// After `git`: anything that rewrites refs or history, or reaches a remote.
const DESTRUCTIVE_GIT: &[&str] = &[
    "push",
    "reset",
    "clean",
    "rebase",
    "checkout",
    "restore",
    "switch",
    "stash",
    "gc",
    "prune",
    "filter-branch",
    "update-ref",
    "config",
    "remote",
    "worktree",
    "branch",
    "tag",
];
/// Flags that make a harmless command destructive.
const DESTRUCTIVE_FLAGS: &[&str] = &[
    "-delete",
    "-exec",
    "--force",
    "--hard",
    "--no-verify",
    "--force-with-lease",
    "--output",
    "-o",
    "-i",
    "--in-place",
];

/// One auto-allow rule.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Rule {
    /// Short stable name (`[A-Za-z0-9_-]`), shown in history.
    pub id: String,
    /// The project whose sessions the rule applies to.
    pub project_id: ProjectId,
    /// Exact tool name, case-sensitive: `Bash` or one of [`PATH_TOOLS`].
    pub tool: String,
    /// `Bash`: the command (exact), or its leading words when `prefix`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub command: Option<String>,
    /// `Bash`: the command may continue with plain, project-relative arguments after a space.
    #[serde(default)]
    pub prefix: bool,
    /// Path tools: the project-relative directory (`""` or `.` = the whole session folder).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dir: Option<String>,
    /// Lets the rule match a destructive command or a secrets path. Never lifts the plain-command rule.
    #[serde(default)]
    pub allow_destructive: bool,
}

/// What the Settings page edits and the daemon persists.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub rules: Vec<Rule>,
}

fn path_key(tool: &str) -> Option<&'static str> {
    PATH_TOOLS.iter().find(|(t, _)| *t == tool).map(|(_, k)| *k)
}

/// One-space-separated words of `[A-Za-z0-9_./:=@+,-]`, no `..` segment.
fn is_plain(text: &str) -> bool {
    !text.is_empty()
        && text.len() <= MAX_FIELD
        && !text.starts_with(' ')
        && !text.ends_with(' ')
        && !text.contains("  ")
        && text
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || " _./:=@+,-".contains(c))
        && !text
            .split(' ')
            .any(|word| word.split('/').any(|part| part == ".."))
}

/// The arguments a prefix rule lets through: plain, no `=` (no `--output=file`), nothing absolute.
fn args_are_plain(args: &str) -> bool {
    is_plain(args) && !args.contains('=') && !args.split(' ').any(|w| w.starts_with('/'))
}

fn basename(word: &str) -> &str {
    word.rsplit('/').next().unwrap_or(word)
}

fn is_destructive(command: &str) -> bool {
    let words: Vec<&str> = command.split(' ').collect();
    let git = words.iter().any(|w| basename(w) == "git");
    words.iter().any(|w| {
        let name = basename(w);
        DESTRUCTIVE_WORDS.contains(&name)
            || DESTRUCTIVE_FLAGS.contains(w)
            || (git && DESTRUCTIVE_GIT.contains(&name))
    })
}

/// A word of `command` names a secrets file or folder (`cat .env`, `ls .git/hooks`).
fn touches_sensitive(command: &str) -> bool {
    command
        .split(' ')
        .any(|word| word.split('/').any(is_sensitive))
}

fn command_matches(rule: &str, prefix: bool, command: &str, allow_destructive: bool) -> bool {
    if !is_plain(command)
        || (!allow_destructive && (is_destructive(command) || touches_sensitive(command)))
    {
        return false;
    }
    if !prefix {
        return command == rule;
    }
    match command.strip_prefix(rule) {
        Some("") => true,
        Some(rest) => rest.strip_prefix(' ').is_some_and(args_are_plain),
        None => false,
    }
}

/// Files and folders that hold secrets or the agent's own configuration (compared case-insensitively:
/// the volume may be).
fn is_sensitive(name: &str) -> bool {
    let n = name.to_ascii_lowercase();
    matches!(
        n.as_str(),
        ".git"
            | ".ssh"
            | ".aws"
            | ".gnupg"
            | ".netrc"
            | ".npmrc"
            | ".claude"
            | ".lotse"
            | ".git-credentials"
            | ".github"
            | ".husky"
            | ".vscode"
            | ".gitlab-ci.yml"
            | ".zshrc"
            | ".bashrc"
            | ".profile"
            | "makefile"
            | "package.json"
            | "build.rs"
    ) || n.starts_with(".env")
        || n.starts_with("id_rsa")
        || n.starts_with("id_ed25519")
        || n.ends_with(".pem")
        || n.ends_with(".key")
}

/// `path` with its longest existing part resolved (symlinks, case), the rest appended as written.
fn lenient(path: &Path) -> PathBuf {
    let mut tail = Vec::new();
    let mut current = path.to_path_buf();
    loop {
        if let Ok(real) = current.canonicalize() {
            return tail.iter().rev().fold(real, |acc, part| acc.join(part));
        }
        match (current.file_name().map(|n| n.to_owned()), current.parent()) {
            (Some(name), Some(parent)) => {
                tail.push(name);
                current = parent.to_path_buf();
            }
            _ => return path.to_path_buf(),
        }
    }
}

fn path_allowed(root: &Path, dir: &str, candidate: &str, allow_sensitive: bool) -> bool {
    let path = Path::new(candidate);
    if candidate.is_empty()
        || candidate.len() > MAX_FIELD * 4
        || candidate.contains('\0')
        || !path.is_absolute()
        || path.components().any(|c| matches!(c, Component::ParentDir))
    {
        return false;
    }
    let Ok(root) = root.canonicalize() else {
        return false;
    };
    let base = if dir.is_empty() || dir == "." {
        root.clone()
    } else {
        lenient(&root.join(dir))
    };
    let target = lenient(path);
    if !base.starts_with(&root) || !target.starts_with(&base) {
        return false;
    }
    allow_sensitive
        || target.strip_prefix(&root).is_ok_and(|rel| {
            !rel.components()
                .any(|c| is_sensitive(&c.as_os_str().to_string_lossy()))
        })
}

impl Rule {
    /// Why this rule cannot be stored, if so.
    pub fn validate(&self) -> Result<(), String> {
        let id_ok = !self.id.is_empty()
            && self.id.len() <= 64
            && self
                .id
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_');
        if !id_ok {
            return Err("a rule id is 1-64 letters, digits, - or _".into());
        }
        if self.tool == "Bash" {
            let Some(command) = self.command.as_deref() else {
                return Err("a Bash rule needs a command".into());
            };
            if self.dir.is_some() {
                return Err("a Bash rule has no directory".into());
            }
            if !is_plain(command) {
                return Err(
                    "a command is plain words of letters, digits and _ . / : = @ + , - (no ; & | $ quotes or newlines)"
                        .into(),
                );
            }
            if self.prefix && command.contains('=') {
                return Err("a prefix rule cannot contain =".into());
            }
            if !self.allow_destructive && is_destructive(command) {
                return Err("that command is destructive: allow it explicitly or drop it".into());
            }
            if !self.allow_destructive && touches_sensitive(command) {
                return Err(
                    "that command names a secrets file or folder: allow it explicitly or drop it"
                        .into(),
                );
            }
            return Ok(());
        }
        if path_key(&self.tool).is_none() {
            return Err(format!("tool {:?} cannot be auto-allowed", self.tool));
        }
        if self.command.is_some() || self.prefix {
            return Err("a path rule has a directory, not a command".into());
        }
        let dir = self.dir.as_deref().unwrap_or("");
        let dir_ok = dir.len() <= MAX_FIELD
            && !dir.starts_with('/')
            && !dir.contains('\0')
            && !Path::new(dir)
                .components()
                .any(|c| matches!(c, Component::ParentDir));
        if !dir_ok {
            return Err("a directory is relative to the project, without ..".into());
        }
        Ok(())
    }

    fn matches(&self, tool: &str, input: &Map<String, Value>, root: &Path) -> bool {
        if tool != self.tool {
            return false;
        }
        if tool == "Bash" {
            // `dangerouslyDisableSandbox` runs the command outside the sandbox: always a person's call
            if input.get("dangerouslyDisableSandbox") == Some(&Value::Bool(true)) {
                return false;
            }
            let (Some(rule), Some(Value::String(command))) =
                (self.command.as_deref(), input.get("command"))
            else {
                return false;
            };
            return command_matches(rule, self.prefix, command, self.allow_destructive);
        }
        let Some(Value::String(path)) = path_key(tool).and_then(|k| input.get(k)) else {
            return false;
        };
        path_allowed(
            root,
            self.dir.as_deref().unwrap_or(""),
            path,
            self.allow_destructive,
        )
    }
}

impl Config {
    /// The first reason this config cannot be stored, if any.
    pub fn validate(&self) -> Result<(), String> {
        if self.rules.len() > MAX_RULES {
            return Err(format!("at most {MAX_RULES} rules"));
        }
        let mut seen = std::collections::HashSet::new();
        for rule in &self.rules {
            rule.validate()
                .map_err(|e| format!("rule {:?}: {e}", rule.id))?;
            if !seen.insert(rule.id.as_str()) {
                return Err(format!("rule id {:?} is used twice", rule.id));
            }
        }
        Ok(())
    }
}

/// The rules in force, and where they are persisted.
pub struct AutoAllow {
    path: PathBuf,
    config: RwLock<Config>,
    seq: AtomicU64,
}

impl AutoAllow {
    /// Read `<data_dir>/autoallow.json`. Never fails: a missing, unreadable, foreign, group- or
    /// world-accessible, oversized or invalid file means "off, no rules".
    pub fn load(data_dir: &Path) -> Self {
        let path = data_dir.join(FILE);
        let config = match read_private(&path) {
            Ok(Some(config)) => config,
            Ok(None) => Config::default(),
            Err(why) => {
                tracing::warn!("auto-allow rules ignored ({}): {why}", path.display());
                Config::default()
            }
        };
        Self {
            path,
            config: RwLock::new(config),
            seq: AtomicU64::new(0),
        }
    }

    pub fn config(&self) -> Config {
        self.config
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
    }

    /// Validate, persist (atomically, 0600), then use `config`. On any error the rules in force stay.
    pub fn set_config(&self, config: Config) -> Result<(), String> {
        config.validate()?;
        let text = serde_json::to_vec_pretty(&config).map_err(|e| e.to_string())?;
        let tmp = self.path.with_extension("json.tmp");
        let write = || -> std::io::Result<()> {
            let mut file = crate::secure::open_private(&tmp)?;
            file.set_len(0)?;
            file.write_all(&text)?;
            file.sync_all()?;
            std::fs::rename(&tmp, &self.path)
        };
        write().map_err(|e| format!("saving the rules: {e}"))?;
        *self.config.write().unwrap_or_else(|e| e.into_inner()) = config;
        Ok(())
    }

    /// The rule that lets `tool` run with `input` in a session of `project` whose folder is `root`.
    pub fn decide(
        &self,
        project: &ProjectId,
        root: &Path,
        tool: &str,
        input: &Map<String, Value>,
    ) -> Option<Rule> {
        let config = self.config.read().unwrap_or_else(|e| e.into_inner());
        if !config.enabled {
            return None;
        }
        config
            .rules
            .iter()
            .find(|r| &r.project_id == project && r.matches(tool, input, root))
            .cloned()
    }

    fn next_prompt_id(&self, at: UnixMs) -> String {
        format!("auto-{at}-{}", self.seq.fetch_add(1, Ordering::Relaxed))
    }
}

/// `Ok(None)`: no file. `Err`: a file that must not be trusted.
fn read_private(path: &Path) -> Result<Option<Config>, String> {
    let meta = match std::fs::symlink_metadata(path) {
        Ok(m) => m,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(e.to_string()),
    };
    if !meta.is_file() {
        return Err("not a regular file".into());
    }
    if meta.uid() != nix::unistd::getuid().as_raw() {
        return Err("not owned by this user".into());
    }
    if meta.mode() & 0o077 != 0 {
        return Err("readable or writable by others".into());
    }
    if meta.len() > MAX_FILE_BYTES {
        return Err("too large".into());
    }
    let text = std::fs::read_to_string(path).map_err(|e| e.to_string())?;
    let config: Config = serde_json::from_str(&text).map_err(|e| e.to_string())?;
    config.validate()?;
    Ok(Some(config))
}

/// Is `payload` a question for [`State::autoallow_query`] (and not a hook event)?
pub fn is_query(payload: &Value) -> bool {
    payload.get("kind").and_then(Value::as_str) == Some(QUERY_KIND)
}

/// What the call is about, for history: the command or the path, capped.
fn subject(input: &Map<String, Value>) -> String {
    ["command", "file_path", "notebook_path", "path"]
        .iter()
        .find_map(|k| input.get(*k).and_then(Value::as_str))
        .unwrap_or("")
        .chars()
        .take(MAX_SUMMARY_CHARS)
        .collect()
}

/// The decision for one question. `Some(rule)` only after the allow is in history; a history that
/// cannot be written is a "no", so no auto-allow goes unrecorded.
pub(crate) fn evaluate(
    auto: &AutoAllow,
    db: &Db,
    session: &SessionId,
    project: &ProjectId,
    root: &Path,
    payload: &Value,
    at: UnixMs,
) -> Option<Rule> {
    let tool = payload.get("tool_name")?.as_str()?;
    let input = payload.get("tool_input")?.as_object()?;
    let rule = auto.decide(project, root, tool, input)?;
    let summary = format!("auto-allow {}: {}", rule.id, subject(input));
    match db.insert_auto_allow(session, &auto.next_prompt_id(at), tool, &summary, at) {
        Ok(()) => Some(rule),
        Err(e) => {
            tracing::warn!("auto-allow not granted, history write failed: {e}");
            None
        }
    }
}

fn refuse(code: ErrorCode, message: &str) -> Response {
    Response::Error(ProtoError {
        code,
        message: message.into(),
    })
}

impl State {
    /// `HookEvent` with `kind: "lotse_autoallow"`: `Response::Ok` = allow this call (it is in history),
    /// any error = ask the person as usual.
    pub(crate) fn autoallow_query(
        &self,
        client: &str,
        session: Option<&SessionId>,
        payload: &Value,
    ) -> Response {
        let no = || refuse(ErrorCode::Conflict, "no auto-allow rule applies");
        if client != HOOK_CLIENT && client != crate::mod_source::MOD_CLIENT {
            return refuse(ErrorCode::InvalidRequest, "only the hook and the mod ask");
        }
        let Some(handle) = session.and_then(|id| self.session(id)) else {
            return no();
        };
        let project = handle.snapshot().project_id;
        let session = handle.snapshot().id;
        match evaluate(
            self.autoallow(),
            self.db(),
            &session,
            &project,
            &handle.cwd(),
            payload,
            now_ms(),
        ) {
            Some(_) => Response::Ok,
            None => no(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use lotse_proto::PermDecision;
    use serde_json::json;
    use std::os::unix::fs::{PermissionsExt, symlink};

    fn p(id: &str) -> ProjectId {
        ProjectId::new(id)
    }

    fn bash(id: &str, command: &str, prefix: bool) -> Rule {
        Rule {
            id: id.into(),
            project_id: p("p1"),
            tool: "Bash".into(),
            command: Some(command.into()),
            prefix,
            dir: None,
            allow_destructive: false,
        }
    }

    fn path_rule(id: &str, tool: &str, dir: &str) -> Rule {
        Rule {
            id: id.into(),
            project_id: p("p1"),
            tool: tool.into(),
            command: None,
            prefix: false,
            dir: Some(dir.into()),
            allow_destructive: false,
        }
    }

    fn on(rules: Vec<Rule>) -> Config {
        Config {
            enabled: true,
            rules,
        }
    }

    fn input(v: Value) -> Map<String, Value> {
        v.as_object().unwrap().clone()
    }

    /// A rule list in force, without touching the disk.
    fn auto_with(config: Config) -> AutoAllow {
        AutoAllow {
            path: PathBuf::from("/nonexistent/autoallow.json"),
            config: RwLock::new(config),
            seq: AtomicU64::new(0),
        }
    }

    fn decides(auto: &AutoAllow, root: &Path, tool: &str, v: Value) -> bool {
        auto.decide(&p("p1"), root, tool, &input(v)).is_some()
    }

    fn runs(auto: &AutoAllow, command: &str) -> bool {
        decides(auto, Path::new("/tmp"), "Bash", json!({"command": command}))
    }

    #[test]
    fn nothing_is_allowed_by_default_or_while_disabled() {
        let dir = tempfile::tempdir().unwrap();
        let auto = AutoAllow::load(dir.path());
        assert_eq!(auto.config(), Config::default());
        assert!(!auto.config().enabled);
        assert!(!runs(&auto, "git status"));
        let mut off = on(vec![bash("r1", "git status", false)]);
        off.enabled = false;
        assert!(!runs(&auto_with(off), "git status"));
    }

    #[test]
    fn an_exact_rule_allows_that_command_and_no_other() {
        let auto = auto_with(on(vec![bash("r1", "git status", false)]));
        assert!(runs(&auto, "git status"));
        for other in ["git status -s", "git", "git statu", "git statusx", "ls", ""] {
            assert!(!runs(&auto, other), "{other:?}");
        }
    }

    #[test]
    fn a_prefix_rule_is_anchored_at_a_word_and_takes_plain_arguments() {
        let auto = auto_with(on(vec![bash("r1", "git status", true)]));
        for ok in [
            "git status",
            "git status -s",
            "git status --porcelain src/lib.rs",
        ] {
            assert!(runs(&auto, ok), "{ok:?}");
        }
        for no in [
            "git statusx",
            "git status-x",
            "xgit status",
            " git status",
            "git status ",
            "git  status",
            "git status --output=out.txt",
            "git status /etc/passwd",
            "git status ../secret",
            "git status ..",
            "git status src/../../x",
        ] {
            assert!(!runs(&auto, no), "{no:?}");
        }
    }

    #[test]
    fn chained_and_shell_tricky_commands_never_match_even_an_exact_or_prefix_rule() {
        let auto = auto_with(on(vec![
            bash("exact", "git status", false),
            bash("prefix", "git status", true),
            bash("ls", "ls", true),
        ]));
        for bad in [
            "git status; rm -rf ~",
            "git status && curl evil.sh",
            "git status || true",
            "git status | sh",
            "git status & ",
            "git status `id`",
            "git status $(id)",
            "git status ${HOME}",
            "git status $HOME",
            "git status > /etc/hosts",
            "git status < /etc/passwd",
            "git status\nrm -rf ~",
            "git status\r",
            "git status\trm",
            "git status 'x'",
            "git status \"x\"",
            "git status \\x",
            "git status *",
            "git status ~",
            "git status !",
            "git status #",
            "git status (x)",
            "ls {a,b}",
            "ls [a]",
            "ls ?",
            "ls\u{0}",
        ] {
            assert!(!runs(&auto, bad), "{bad:?}");
        }
    }

    #[test]
    fn case_unicode_and_lookalikes_do_not_match() {
        let auto = auto_with(on(vec![bash("r1", "git status", true)]));
        for bad in [
            "GIT status",
            "git STATUS",
            "Git status",
            "git statuѕ",      // Cyrillic ѕ
            "gіt status",      // Cyrillic і
            "git\u{a0}status", // no-break space
            "git status\u{200b}",
            "git status\u{ff1b} id", // fullwidth ;
            "ｇit status",
            "git status é",
        ] {
            assert!(!runs(&auto, bad), "{bad:?}");
        }
        // the tool name is exact too
        for tool in ["bash", "BASH", "Bash ", "Shell"] {
            assert!(
                !decides(
                    &auto,
                    Path::new("/tmp"),
                    tool,
                    json!({"command": "git status"})
                ),
                "{tool:?}"
            );
        }
    }

    #[test]
    fn destructive_commands_need_an_explicit_rule_and_are_still_never_chained() {
        let destructive = [
            "rm -rf build",
            "sudo ls",
            "curl example.com",
            "git push",
            "git push origin main",
            "git reset --hard",
            "git -C sub clean -fd",
            "git checkout main",
            "/bin/rm file",
            "find . -delete",
            "ls --force",
            "docker ps",
            "tee out.txt",
        ];
        for command in destructive {
            let mut rule = bash("r1", command, false);
            assert!(rule.validate().is_err(), "{command:?} validates");
            // the matcher refuses it even if a rule got in without validation (a hand-edited file)
            let auto = auto_with(on(vec![rule.clone()]));
            assert!(!runs(&auto, command), "{command:?} matches");
            rule.allow_destructive = true;
            assert!(rule.validate().is_ok(), "{command:?}");
            let auto = auto_with(on(vec![rule]));
            assert!(runs(&auto, command), "{command:?} explicit");
            assert!(
                !runs(&auto, &format!("{command}; ls")),
                "{command:?} chained"
            );
        }
        // an explicit prefix rule does not reach other destructive commands
        let mut rule = bash("r1", "git push", true);
        rule.allow_destructive = true;
        let auto = auto_with(on(vec![rule]));
        assert!(runs(&auto, "git push origin main"));
        assert!(!runs(&auto, "git push origin main && rm x"));
        assert!(
            !runs(&auto, "git push origin main --force=1"),
            "= is not a plain argument"
        );
        assert!(!runs(&auto, "git pull"), "another command");
    }

    #[test]
    fn a_command_cannot_reach_secrets_through_a_harmless_prefix_rule() {
        let auto = auto_with(on(vec![bash("r1", "cat", true), bash("r2", "ls", true)]));
        for ok in [
            "cat src/a.rs",
            "cat README.md",
            "ls -la docs",
            "cat .gitignore",
        ] {
            assert!(runs(&auto, ok), "{ok:?}");
        }
        for bad in [
            "cat .env",
            "cat .ENV",
            "cat src/.env.local",
            "cat .git/config",
            "cat .Git/config",
            "cat .ssh/id_rsa",
            "cat id_ed25519",
            "cat server.pem",
            "cat api.key",
            "ls .git",
            "ls .claude/settings.json",
            "cat .npmrc",
        ] {
            assert!(!runs(&auto, bad), "{bad:?}");
        }
        // an exact rule naming a secret is not stored, unless explicit
        let mut rule = bash("r3", "cat .env", false);
        assert!(rule.validate().is_err());
        rule.allow_destructive = true;
        assert!(rule.validate().is_ok());
        assert!(runs(&auto_with(on(vec![rule])), "cat .env"));
    }

    #[test]
    fn a_command_run_outside_the_sandbox_is_always_a_persons_call() {
        let auto = auto_with(on(vec![bash("r1", "ls", false)]));
        let call = |v| decides(&auto, Path::new("/tmp"), "Bash", v);
        assert!(call(json!({"command": "ls"})));
        assert!(call(
            json!({"command": "ls", "dangerouslyDisableSandbox": false})
        ));
        assert!(!call(
            json!({"command": "ls", "dangerouslyDisableSandbox": true})
        ));
        assert!(!call(json!({"command": 5})));
        assert!(!call(json!({})));
    }

    #[test]
    fn rules_that_could_never_be_safe_are_not_stored() {
        let long = bash("r1", &"a".repeat(MAX_FIELD + 1), false);
        let cases: Vec<(Rule, &str)> = vec![
            (bash("", "ls", false), "empty id"),
            (bash("a b", "ls", false), "id with a space"),
            (bash(&"x".repeat(65), "ls", false), "long id"),
            (bash("r", "", false), "empty command"),
            (bash("r", "ls; id", false), "semicolon"),
            (bash("r", "ls | wc", false), "pipe"),
            (bash("r", "echo $HOME", false), "dollar"),
            (bash("r", "ls\n", false), "newline"),
            (bash("r", "ls ../..", false), "dotdot"),
            (bash("r", "ls  -l", false), "two spaces"),
            (bash("r", " ls", false), "leading space"),
            (bash("r", "ls é", false), "non-ascii"),
            (bash("r", "FOO=1 ls", true), "= in a prefix rule"),
            (long, "long command"),
            (path_rule("r", "Bash", "src"), "Bash with a dir"),
            (path_rule("r", "WebFetch", "src"), "unknown tool"),
            (path_rule("r", "Read", "../x"), "dotdot dir"),
            (path_rule("r", "Read", "src/../../x"), "nested dotdot dir"),
            (path_rule("r", "Read", "/etc"), "absolute dir"),
            (
                Rule {
                    command: Some("ls".into()),
                    ..path_rule("r", "Read", "src")
                },
                "path rule with a command",
            ),
            (
                Rule {
                    dir: Some("src".into()),
                    ..bash("r", "ls", false)
                },
                "bash rule with a dir",
            ),
        ];
        for (rule, what) in cases {
            assert!(rule.validate().is_err(), "{what}");
            assert!(on(vec![rule]).validate().is_err(), "{what} in a config");
        }
        assert!(
            bash("r", "git status --porcelain", false)
                .validate()
                .is_ok()
        );
        assert!(path_rule("r", "Read", "").validate().is_ok());
        let twice = on(vec![bash("r", "ls", false), bash("r", "pwd", false)]);
        assert!(twice.validate().is_err(), "duplicate id");
        let many = on((0..=MAX_RULES)
            .map(|i| bash(&format!("r{i}"), "ls", false))
            .collect());
        assert!(many.validate().is_err(), "too many");
    }

    /// `<tmp>/proj/{src,docs,.git}`, `<tmp>/proj2`, `<tmp>/outside`, a link `proj/src/out -> outside`.
    fn tree() -> (tempfile::TempDir, PathBuf) {
        let tmp = tempfile::tempdir().unwrap();
        let base = tmp.path().canonicalize().unwrap();
        let root = base.join("proj");
        for dir in ["proj/src", "proj/docs", "proj/.git", "proj2", "outside"] {
            std::fs::create_dir_all(base.join(dir)).unwrap();
        }
        std::fs::write(base.join("proj/src/a.rs"), "x").unwrap();
        std::fs::write(base.join("outside/secret"), "x").unwrap();
        symlink(base.join("outside"), root.join("src/out")).unwrap();
        (tmp, root)
    }

    fn reads(auto: &AutoAllow, root: &Path, tool: &str, key: &str, path: &str) -> bool {
        decides(auto, root, tool, json!({ key: path }))
    }

    #[test]
    fn a_path_rule_covers_its_folder_inside_the_session_folder_only() {
        let (_tmp, root) = tree();
        let auto = auto_with(on(vec![
            path_rule("r1", "Read", "src"),
            path_rule("r2", "Edit", ""),
        ]));
        let at = |rel: &str| root.join(rel).to_string_lossy().into_owned();
        let read = |path: &str| reads(&auto, &root, "Read", "file_path", path);
        let edit = |path: &str| reads(&auto, &root, "Edit", "file_path", path);
        assert!(read(&at("src/a.rs")));
        assert!(
            read(&at("src/new/file.rs")),
            "a file that does not exist yet"
        );
        assert!(read(&at("src")));
        assert!(!read(&at("docs/a.md")), "another folder of the project");
        assert!(!read(&at("a.md")), "the project root is not src");
        assert!(
            edit(&at("docs/a.md")) && edit(&at("README.md")),
            "dir '' = whole project"
        );
        // other tools do not borrow the rule, and keys other than the tool's own are ignored
        assert!(!reads(&auto, &root, "Write", "file_path", &at("src/a.rs")));
        assert!(!reads(&auto, &root, "Read", "path", &at("src/a.rs")));
        assert!(!decides(&auto, &root, "Read", json!({})));
        assert!(!decides(&auto, &root, "Read", json!({"file_path": 7})));
    }

    #[test]
    fn paths_that_leave_the_project_or_wander_never_match() {
        let (tmp, root) = tree();
        let base = tmp.path().canonicalize().unwrap();
        let auto = auto_with(on(vec![path_rule("r2", "Read", "")]));
        let read = |path: &str| reads(&auto, &root, "Read", "file_path", path);
        let s = |p: PathBuf| p.to_string_lossy().into_owned();
        for bad in [
            "/etc/passwd".to_string(),
            "/".to_string(),
            s(base.join("proj2/x")), // shares the name prefix
            s(base.join("outside/secret")),
            s(root.join("src/../../outside/secret")), // traversal
            s(root.join("src/../src/a.rs")),          // any .. at all
            s(root.join("../proj/src/a.rs")),
            format!("{}/..", root.display()),
            s(root.join("src/out/secret")), // symlink out of the project
            s(root.join("src/out")),
            "src/a.rs".to_string(), // relative
            "./src/a.rs".to_string(),
            "~/x".to_string(),
            "".to_string(),
            format!("{}/src/a.rs\0.txt", root.display()),
        ] {
            assert!(!read(&bad), "{bad:?}");
        }
        // a root that does not exist matches nothing
        assert!(!reads(
            &auto,
            &root.join("gone"),
            "Read",
            "file_path",
            &s(root.join("gone/x"))
        ));
        // a dir rule that is itself a symlink out of the project is nothing
        let linked = auto_with(on(vec![path_rule("r3", "Read", "src/out")]));
        assert!(!reads(
            &linked,
            &root,
            "Read",
            "file_path",
            &s(base.join("outside/secret"))
        ));
        assert!(!reads(
            &linked,
            &root,
            "Read",
            "file_path",
            &s(root.join("src/out/secret"))
        ));
    }

    #[test]
    fn secrets_and_agent_config_are_not_covered_unless_the_rule_says_so() {
        let (_tmp, root) = tree();
        let at = |rel: &str| root.join(rel).to_string_lossy().into_owned();
        let auto = auto_with(on(vec![path_rule("r", "Read", "")]));
        let read = |a: &AutoAllow, path: &str| reads(a, &root, "Read", "file_path", path);
        for bad in [
            ".env",
            ".env.local",
            ".ENV",
            ".envrc",
            ".git/config",
            ".GIT/hooks/pre-commit",
            ".Git",
            "src/.ssh/id_rsa",
            ".claude/settings.json",
            ".aws/credentials",
            "src/id_ed25519",
            "src/server.pem",
            "docs/api.key",
            ".npmrc",
            ".netrc",
        ] {
            assert!(!read(&auto, &at(bad)), "{bad:?}");
        }
        assert!(read(&auto, &at(".gitignore")));
        // files that run later (CI, hooks, build scripts) need an explicit rule too
        assert!(!read(&auto, &at(".github/ci.yml")) && !read(&auto, &at("package.json")));
        let mut explicit = path_rule("r", "Read", "");
        explicit.allow_destructive = true;
        let auto = auto_with(on(vec![explicit]));
        assert!(read(&auto, &at(".env")) && read(&auto, &at(".git/config")));
        assert!(
            !read(&auto, &at("../x")),
            "explicit does not lift the project boundary"
        );
    }

    #[test]
    fn a_rule_applies_to_the_sessions_of_its_project_only() {
        let auto = auto_with(on(vec![bash("r1", "git status", false)]));
        let input = input(json!({"command": "git status"}));
        let root = Path::new("/tmp");
        assert!(auto.decide(&p("p1"), root, "Bash", &input).is_some());
        assert!(auto.decide(&p("p2"), root, "Bash", &input).is_none());
        assert!(auto.decide(&p("P1"), root, "Bash", &input).is_none());
    }

    #[test]
    fn the_rules_survive_a_restart_in_a_private_file_of_the_data_dir() {
        let dir = tempfile::tempdir().unwrap();
        let auto = AutoAllow::load(dir.path());
        let config = on(vec![
            bash("r1", "git status", true),
            path_rule("r2", "Read", "src"),
        ]);
        auto.set_config(config.clone()).unwrap();
        let file = dir.path().join(FILE);
        assert_eq!(
            std::fs::metadata(&file).unwrap().permissions().mode() & 0o777,
            0o600
        );
        assert!(!dir.path().join("autoallow.json.tmp").exists());
        assert_eq!(AutoAllow::load(dir.path()).config(), config);
        assert!(runs(&AutoAllow::load(dir.path()), "git status -s"));

        // a rejected config changes nothing, in memory or on disk
        let bad = on(vec![bash("r9", "ls; id", false)]);
        assert!(auto.set_config(bad).is_err());
        assert_eq!(auto.config(), config);
        assert_eq!(AutoAllow::load(dir.path()).config(), config);
    }

    #[test]
    fn a_file_that_cannot_be_trusted_means_off() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join(FILE);
        let good = serde_json::to_string(&on(vec![bash("r1", "git status", false)])).unwrap();
        let load = |text: &str, mode: u32| {
            std::fs::write(&file, text).unwrap();
            std::fs::set_permissions(&file, std::fs::Permissions::from_mode(mode)).unwrap();
            AutoAllow::load(dir.path()).config()
        };
        assert_eq!(load(&good, 0o600).rules.len(), 1);
        assert_eq!(load(&good, 0o644), Config::default(), "world-readable");
        assert_eq!(load(&good, 0o660), Config::default(), "group-writable");
        assert_eq!(load("not json", 0o600), Config::default());
        assert_eq!(
            load(r#"{"enabled":true,"rules":[],"extra":1}"#, 0o600),
            Config::default()
        );
        let hand_edited = r#"{"enabled":true,"rules":[{"id":"r","project_id":"p1","tool":"Bash","command":"ls; id"}]}"#;
        assert_eq!(
            load(hand_edited, 0o600),
            Config::default(),
            "an unsafe rule voids the file"
        );
        let oversized = " ".repeat(MAX_FILE_BYTES as usize + 1);
        assert_eq!(load(&oversized, 0o600), Config::default());
        // a link in place of the file is not followed
        std::fs::remove_file(&file).unwrap();
        let elsewhere = dir.path().join("elsewhere.json");
        std::fs::write(&elsewhere, &good).unwrap();
        std::fs::set_permissions(&elsewhere, std::fs::Permissions::from_mode(0o600)).unwrap();
        symlink(&elsewhere, &file).unwrap();
        assert_eq!(AutoAllow::load(dir.path()).config(), Config::default());
    }

    fn query(tool: &str, input: Value) -> Value {
        json!({"kind": QUERY_KIND, "tool_name": tool, "tool_input": input})
    }

    #[test]
    fn an_allow_is_written_to_history_before_it_is_given() {
        let db = Db::open_in_memory().unwrap();
        let auto = auto_with(on(vec![bash("r1", "git status", false)]));
        let s = SessionId::new("s1");
        let root = Path::new("/tmp");
        let at = 1_000;
        let ask = |payload: &Value, project: &str| {
            evaluate(&auto, &db, &s, &p(project), root, payload, at)
        };

        assert!(is_query(&query("Bash", json!({}))));
        assert!(!is_query(&json!({"hook_event_name": "PreToolUse"})));

        let rule = ask(&query("Bash", json!({"command": "git status"})), "p1").unwrap();
        assert_eq!(rule.id, "r1");
        let rows = db.list_perm_events(&s).unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].tool, "Bash");
        assert_eq!(rows[0].summary, "auto-allow r1: git status");
        assert_eq!(rows[0].decision, Some(PermDecision::Allow));
        assert_eq!((rows[0].asked_at, rows[0].decided_at), (at, Some(at)));
        // each grant is its own row
        assert!(ask(&query("Bash", json!({"command": "git status"})), "p1").is_some());
        assert_eq!(db.list_perm_events(&s).unwrap().len(), 2);

        // refusals leave no row and no trace
        let before = db.list_perm_events(&s).unwrap().len();
        for (payload, project) in [
            (query("Bash", json!({"command": "git status; id"})), "p1"),
            (query("Bash", json!({"command": "git status"})), "other"),
            (query("Read", json!({"file_path": "/etc/passwd"})), "p1"),
            (json!({"kind": QUERY_KIND}), "p1"),
            (
                json!({"kind": QUERY_KIND, "tool_name": "Bash", "tool_input": "git status"}),
                "p1",
            ),
            (
                json!({"kind": QUERY_KIND, "tool_name": 5, "tool_input": {}}),
                "p1",
            ),
        ] {
            assert!(ask(&payload, project).is_none(), "{payload}");
        }
        assert_eq!(db.list_perm_events(&s).unwrap().len(), before);
    }

    #[test]
    fn a_long_command_is_summarised_in_history_but_matched_whole() {
        let db = Db::open_in_memory().unwrap();
        let long = format!("git status {}", "a".repeat(400));
        let auto = auto_with(on(vec![bash("r1", "git status", true)]));
        let s = SessionId::new("s1");
        let root = Path::new("/tmp");
        let rule = evaluate(
            &auto,
            &db,
            &s,
            &p("p1"),
            root,
            &query("Bash", json!({"command": long})),
            1,
        );
        assert!(rule.is_some());
        let summary = db.list_perm_events(&s).unwrap().remove(0).summary;
        assert_eq!(
            summary.chars().count(),
            "auto-allow r1: ".len() + MAX_SUMMARY_CHARS
        );
    }
}
