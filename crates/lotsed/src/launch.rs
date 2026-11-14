//! Launch commands: tokenizing with `$SHELL` expansion, PATH lookup, version detection (3 s timeout)
//! and the login-shell environment the sessions run in.

use lotse_proto::{AgentKind, LaunchCommandInfo, Settings};
use std::collections::HashMap;
use std::io::Read;
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::time::{Duration, Instant};

/// `claude --version` and friends must answer within this.
pub const VERSION_TIMEOUT: Duration = Duration::from_secs(3);
/// An interactive login shell with a heavy `.zshrc` (nvm, …) needs longer than a version probe.
/// A failed capture is not retried: the daemon falls back to its own environment until restart.
const LOGIN_ENV_TIMEOUT: Duration = Duration::from_secs(10);
const FALLBACK_SHELL: &str = "/bin/zsh";
/// Variables of the capturing shell that must not leak into sessions.
const ENV_SKIP: &[&str] = &["PWD", "OLDPWD", "SHLVL", "_"];

/// The launch command string configured for `agent` in settings.
pub fn configured_command(settings: &Settings, agent: AgentKind) -> &str {
    let lc = &settings.launch_commands;
    match agent {
        AgentKind::Terminal => &lc.terminal,
        AgentKind::ClaudeCode => &lc.claude_code,
        AgentKind::Codex => &lc.codex,
        AgentKind::OpenCode => &lc.opencode,
    }
}

/// Split a launch command into argv. Supports single and double quotes, backslash escapes,
/// `$VAR` / `${VAR}` outside single quotes and a leading `~`. It is not a shell: no pipes or globs.
pub fn tokenize(cmd: &str, var: &dyn Fn(&str) -> Option<String>) -> Result<Vec<String>, String> {
    let chars: Vec<char> = cmd.chars().collect();
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut has = false;
    let (mut single, mut double) = (false, false);
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        i += 1;
        if single {
            if c == '\'' {
                single = false;
            } else {
                cur.push(c);
            }
            continue;
        }
        match c {
            '\\' if !single => {
                let n = *chars.get(i).ok_or("trailing backslash")?;
                i += 1;
                if double && !matches!(n, '"' | '\\' | '$') {
                    cur.push('\\');
                }
                cur.push(n);
                has = true;
            }
            '"' => {
                double = !double;
                has = true;
            }
            '\'' if !double => {
                single = true;
                has = true;
            }
            '$' => {
                let braced = chars.get(i) == Some(&'{');
                let start = i + usize::from(braced);
                let mut end = start;
                while end < chars.len() && (chars[end].is_ascii_alphanumeric() || chars[end] == '_')
                {
                    end += 1;
                }
                let name: String = chars[start..end].iter().collect();
                if name.is_empty() || name.starts_with(|c: char| c.is_ascii_digit()) {
                    cur.push('$');
                    has = true;
                    continue;
                }
                if braced {
                    if chars.get(end) != Some(&'}') {
                        return Err("unterminated ${".into());
                    }
                    end += 1;
                }
                cur.push_str(&var(&name).unwrap_or_default());
                has = true;
                i = end;
            }
            '~' if !double && !has && matches!(chars.get(i), None | Some('/') | Some(' ')) => {
                cur.push_str(&var("HOME").unwrap_or_else(|| "~".into()));
                has = true;
            }
            c if c.is_whitespace() && !double => {
                if has {
                    out.push(std::mem::take(&mut cur));
                    has = false;
                }
            }
            c => {
                cur.push(c);
                has = true;
            }
        }
    }
    if single || double {
        return Err("unterminated quote".into());
    }
    if has {
        out.push(cur);
    }
    if out.is_empty() {
        return Err("empty command".into());
    }
    Ok(out)
}

/// Claude Code reads `env.CLAUDE_CODE_PLUGIN_DIRS` from a user's settings.json over the process
/// environment (seen with 2.1.311), which would drop Lotse's per-pane hooks: the flag cannot be
/// overridden. Appended last so a positional prompt is never read as a plugin path; the user's own
/// arguments stay as they are.
pub fn add_claude_plugin_dir(argv: &mut Vec<String>, agent: AgentKind, plugin: Option<&Path>) {
    let (AgentKind::ClaudeCode, Some(plugin)) = (agent, plugin) else {
        return;
    };
    // only a program that is Claude Code: a wrapper or a stand-in would take the flag as its own
    let is_claude = argv
        .first()
        .and_then(|p| Path::new(p).file_name())
        .is_some_and(|n| n.to_string_lossy().contains("claude"));
    if !is_claude {
        return;
    }
    let plugin = plugin.to_string_lossy();
    let present = argv
        .windows(2)
        .any(|w| w[0] == "--plugin-dir" && w[1] == plugin);
    if !present {
        argv.push("--plugin-dir".into());
        argv.push(plugin.into_owned());
    }
}

/// Look a program up: a path with `/` is checked as is, a bare name is searched on `path_var`.
pub fn resolve_program(program: &str, path_var: Option<&str>, cwd: &Path) -> Option<PathBuf> {
    if program.contains('/') {
        let p = if Path::new(program).is_absolute() {
            PathBuf::from(program)
        } else {
            cwd.join(program)
        };
        return is_executable(&p).then_some(p);
    }
    which::which_in(program, path_var, cwd).ok()
}

fn is_executable(p: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    std::fs::metadata(p).is_ok_and(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
}

/// Run `program args`, capture stdout (at most `max_bytes`), kill the whole process group on
/// timeout. `None` on spawn failure, timeout or a non-zero exit.
pub fn run_capture(
    program: &Path,
    args: &[&str],
    env: Option<&HashMap<String, String>>,
    timeout: Duration,
    max_bytes: u64,
) -> Option<String> {
    let mut cmd = Command::new(program);
    cmd.args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .process_group(0);
    if let Some(env) = env {
        cmd.env_clear().envs(env);
    }
    let mut child = cmd.spawn().ok()?;
    let mut stdout = child.stdout.take()?;
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let mut s = Vec::new();
        let _ = (&mut stdout).take(max_bytes).read_to_end(&mut s);
        let _ = tx.send(s);
    });
    let deadline = Instant::now() + timeout;
    let status = loop {
        match child.try_wait() {
            Ok(Some(st)) => break st,
            Ok(None) if Instant::now() < deadline => std::thread::sleep(Duration::from_millis(10)),
            _ => {
                kill_group(child.id());
                let _ = child.kill();
                let _ = child.wait();
                return None;
            }
        }
    };
    let bytes = rx
        .recv_timeout(
            deadline.saturating_duration_since(Instant::now()) + Duration::from_millis(200),
        )
        .ok()?;
    status
        .success()
        .then(|| String::from_utf8_lossy(&bytes).into_owned())
}

fn kill_group(pid: u32) {
    use nix::sys::signal::{Signal, killpg};
    let _ = killpg(nix::unistd::Pid::from_raw(pid as i32), Signal::SIGKILL);
}

/// First token of `--version` output that looks like a version (`2.1.309`, `5.9`, `3.2.57(1)-release`).
fn find_version(output: &str) -> Option<String> {
    let line = output.lines().map(str::trim).find(|l| !l.is_empty())?;
    line.split_whitespace().find_map(|tok| {
        let tok = tok.trim_matches(|c| c == ',' || c == ';');
        let tok = tok.strip_prefix(['v', 'V']).unwrap_or(tok);
        let tok = tok.split('(').next().unwrap_or(tok);
        let ok = tok.starts_with(|c: char| c.is_ascii_digit())
            && tok.contains('.')
            && tok
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '+' | '_'));
        ok.then(|| tok.to_owned())
    })
}

/// `zsh 5.9` for shells, `v2.1.309` for agents.
pub fn detect_version(
    agent: AgentKind,
    resolved: &Path,
    env: &HashMap<String, String>,
) -> Option<String> {
    let out = run_capture(resolved, &["--version"], Some(env), VERSION_TIMEOUT, 8192)?;
    let ver = find_version(&out)?;
    Some(match agent {
        AgentKind::Terminal => {
            let name = resolved.file_name()?.to_string_lossy();
            format!("{name} {ver}")
        }
        _ => format!("v{ver}"),
    })
}

/// Environment sessions run in: the user's login-shell environment (or the daemon's own when it
/// cannot be captured) with Lotse's variables on top.
#[derive(Debug, Clone, Default)]
pub struct LaunchEnv {
    vars: HashMap<String, String>,
}

impl LaunchEnv {
    /// The daemon's own environment, no shell involved (tests, fallback).
    pub fn from_process() -> Self {
        Self::from_vars(std::env::vars().collect())
    }

    /// `vars` without the markers of an agent Lotse may have been started from (see
    /// [`crate::agent_env`]).
    pub fn from_vars(mut vars: HashMap<String, String>) -> Self {
        crate::agent_env::scrub_host_markers(&mut vars);
        Self { vars }
    }

    /// Daemon environment overlaid with what `$SHELL -l -i` exports. Blocking, at most ~3 s.
    pub fn from_login_shell() -> Self {
        let mut env = Self::from_process();
        let shell = env.shell();
        match capture_login_env(Path::new(&shell)) {
            Some(captured) => {
                env.vars.extend(captured);
                // the shell was started with the daemon's environment: its copies are back
                crate::agent_env::scrub_host_markers(&mut env.vars);
            }
            None => tracing::warn!("login shell environment not captured, using the daemon's own"),
        }
        env
    }

    pub fn shell(&self) -> String {
        self.vars
            .get("SHELL")
            .filter(|s| !s.is_empty())
            .cloned()
            .unwrap_or_else(|| FALLBACK_SHELL.into())
    }

    pub fn get(&self, name: &str) -> Option<String> {
        if name == "SHELL" {
            return Some(self.shell());
        }
        self.vars.get(name).cloned()
    }

    pub fn path(&self) -> Option<&str> {
        self.vars.get("PATH").map(String::as_str)
    }

    pub fn vars(&self) -> &HashMap<String, String> {
        &self.vars
    }

    pub fn tokenize(&self, cmd: &str) -> Result<Vec<String>, String> {
        tokenize(cmd, &|n| self.get(n))
    }

    /// Resolve the program of `argv`; `None` when not on PATH.
    pub fn resolve(&self, argv: &[String], cwd: &Path) -> Option<PathBuf> {
        resolve_program(argv.first()?, self.path(), cwd)
    }
}

const BEGIN: &str = "__LOTSE_ENV_BEGIN__";
const END: &str = "__LOTSE_ENV_END__";

fn capture_login_env(shell: &Path) -> Option<HashMap<String, String>> {
    let script = format!("printf '\\n{BEGIN}\\n'; env; printf '{END}\\n'");
    let base: HashMap<String, String> = std::env::vars().collect();
    let out = run_capture(
        shell,
        &["-l", "-i", "-c", &script],
        Some(&base),
        LOGIN_ENV_TIMEOUT,
        1 << 20,
    )?;
    parse_env_block(&out)
}

/// Variables between the markers; lines that are not `NAME=` continue the previous value.
fn parse_env_block(out: &str) -> Option<HashMap<String, String>> {
    let body = out.split_once(BEGIN)?.1.split_once(END)?.0;
    let mut vars: Vec<(String, String)> = Vec::new();
    for line in body.lines() {
        let is_assign = line.split_once('=').is_some_and(|(k, _)| {
            !k.is_empty()
                && !k.starts_with(|c: char| c.is_ascii_digit())
                && k.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
        });
        if is_assign {
            let (k, v) = line.split_once('=').unwrap();
            vars.push((k.to_owned(), v.to_owned()));
        } else if let Some(last) = vars.last_mut() {
            last.1.push('\n');
            last.1.push_str(line);
        }
    }
    let map: HashMap<_, _> = vars
        .into_iter()
        .map(|(k, v)| (k, v.trim_end_matches('\n').to_owned()))
        .filter(|(k, _)| !ENV_SKIP.contains(&k.as_str()))
        .collect();
    (!map.is_empty()).then_some(map)
}

/// Settings › Agents rows: configured command, default, found on PATH, version. The four version
/// probes run in parallel so the call stays under one timeout.
pub fn detect_all(settings: &Settings, env: &LaunchEnv) -> Vec<LaunchCommandInfo> {
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("/"));
    std::thread::scope(|s| {
        let handles: Vec<_> = AgentKind::ALL
            .iter()
            .map(|&agent| {
                let cwd = &cwd;
                s.spawn(move || {
                    let command = configured_command(settings, agent).to_owned();
                    let resolved = env
                        .tokenize(&command)
                        .ok()
                        .and_then(|argv| env.resolve(&argv, cwd));
                    let version = resolved
                        .as_deref()
                        .and_then(|p| detect_version(agent, p, env.vars()));
                    LaunchCommandInfo {
                        agent,
                        default_command: agent.default_command().into(),
                        command,
                        found: resolved.is_some(),
                        resolved_path: resolved.map(|p| p.to_string_lossy().into_owned()),
                        version,
                        // hooks / OSC 7501 state comes with the detection WPs
                        integration: None,
                    }
                })
            })
            .collect();
        handles.into_iter().filter_map(|h| h.join().ok()).collect()
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn vars(n: &str) -> Option<String> {
        match n {
            "SHELL" => Some("/bin/zsh".into()),
            "HOME" => Some("/Users/x".into()),
            _ => None,
        }
    }

    fn tok(s: &str) -> Vec<String> {
        tokenize(s, &vars).unwrap()
    }

    #[test]
    fn default_terminal_command_expands_shell() {
        assert_eq!(tok("$SHELL -l"), ["/bin/zsh", "-l"]);
        assert_eq!(tok("${SHELL} -l"), ["/bin/zsh", "-l"]);
    }

    #[test]
    fn quotes_escapes_and_home() {
        assert_eq!(
            tok(r#"claude --model "opus 4" 'a b'"#),
            ["claude", "--model", "opus 4", "a b"]
        );
        assert_eq!(tok(r"a\ b c"), ["a b", "c"]);
        assert_eq!(tok("'$SHELL' \"$SHELL\""), ["$SHELL", "/bin/zsh"]);
        assert_eq!(tok("~/bin/claude"), ["/Users/x/bin/claude"]);
        assert_eq!(tok("x \"\" y"), ["x", "", "y"]);
        assert_eq!(tok("a $UNSET b"), ["a", "", "b"]);
        assert_eq!(tok("price $5"), ["price", "$5"]);
    }

    #[test]
    fn claude_gets_the_lotse_plugin_dir_after_its_own_args() {
        let plugin = Path::new("/data/integrations/claude/lotse-status");
        let mut argv = tok(r#"/bin/claude --model "opus 4" "fix it""#);
        add_claude_plugin_dir(&mut argv, AgentKind::ClaudeCode, Some(plugin));
        assert_eq!(
            argv,
            [
                "/bin/claude",
                "--model",
                "opus 4",
                "fix it",
                "--plugin-dir",
                "/data/integrations/claude/lotse-status"
            ]
        );
        // a user's own --plugin-dir stays; ours is added next to it, once
        let mut own = tok("/bin/claude --plugin-dir /mine");
        add_claude_plugin_dir(&mut own, AgentKind::ClaudeCode, Some(plugin));
        add_claude_plugin_dir(&mut own, AgentKind::ClaudeCode, Some(plugin));
        assert_eq!(
            own,
            [
                "/bin/claude",
                "--plugin-dir",
                "/mine",
                "--plugin-dir",
                "/data/integrations/claude/lotse-status"
            ]
        );
    }

    #[test]
    fn other_agents_and_a_missing_plugin_leave_the_command_alone() {
        let plugin = Path::new("/p");
        for agent in [AgentKind::Terminal, AgentKind::Codex, AgentKind::OpenCode] {
            let mut argv = tok("tool -x");
            add_claude_plugin_dir(&mut argv, agent, Some(plugin));
            assert_eq!(argv, ["tool", "-x"]);
        }
        let mut argv = tok("claude");
        add_claude_plugin_dir(&mut argv, AgentKind::ClaudeCode, None);
        assert_eq!(argv, ["claude"]);
    }

    #[test]
    fn bad_commands_are_errors() {
        assert!(tokenize("", &vars).is_err());
        assert!(tokenize("   ", &vars).is_err());
        assert!(tokenize("a 'b", &vars).is_err());
        assert!(tokenize("a ${B", &vars).is_err());
        assert!(tokenize("a\\", &vars).is_err());
    }

    #[test]
    fn version_parsing() {
        assert_eq!(
            find_version("2.1.309 (Claude Code)\n").as_deref(),
            Some("2.1.309")
        );
        assert_eq!(
            find_version("zsh 5.9 (arm64-apple-darwin24.0)").as_deref(),
            Some("5.9")
        );
        assert_eq!(
            find_version("GNU bash, version 3.2.57(1)-release (arm64-apple-darwin25)").as_deref(),
            Some("3.2.57")
        );
        assert_eq!(find_version("codex-cli v0.46.0").as_deref(), Some("0.46.0"));
        assert_eq!(find_version("no digits here"), None);
        assert_eq!(find_version(""), None);
    }

    #[test]
    fn env_block_parsing_handles_multiline_and_skips_noise() {
        let out = "motd noise\n\n__LOTSE_ENV_BEGIN__\nPATH=/a:/b\nMULTI=one\ntwo\nPWD=/x\n_=/usr/bin/env\nHOME=/h\n\n__LOTSE_ENV_END__\n";
        let m = parse_env_block(out).unwrap();
        assert_eq!(m["PATH"], "/a:/b");
        assert_eq!(m["MULTI"], "one\ntwo");
        assert_eq!(m["HOME"], "/h");
        assert!(!m.contains_key("PWD") && !m.contains_key("_"));
        assert!(parse_env_block("nothing").is_none());
    }

    #[test]
    fn resolve_finds_absolute_and_path_programs() {
        let cwd = Path::new("/");
        assert!(resolve_program("/bin/sh", None, cwd).is_some());
        assert!(resolve_program("/definitely/not/here", None, cwd).is_none());
        assert!(resolve_program("sh", Some("/bin:/usr/bin"), cwd).is_some());
        assert!(resolve_program("lotse-no-such-tool", Some("/bin:/usr/bin"), cwd).is_none());
    }

    #[test]
    fn run_capture_times_out_and_kills() {
        let started = Instant::now();
        let out = run_capture(
            Path::new("/bin/sleep"),
            &["10"],
            None,
            Duration::from_millis(300),
            100,
        );
        assert!(out.is_none());
        assert!(started.elapsed() < Duration::from_secs(3));
        let out = run_capture(
            Path::new("/bin/echo"),
            &["hi"],
            None,
            Duration::from_secs(3),
            100,
        );
        assert_eq!(out.as_deref(), Some("hi\n"));
    }
}
