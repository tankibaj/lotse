//! Beta (wp08): MCP connectors of the agents: `list_connectors`, `check_connectors`.
//!
//! Read from the agents' own config files: Claude Code `.mcp.json` of the session's folder and
//! `~/.claude.json` (`mcpServers`, `projects[<folder>].mcpServers`), Codex `~/.codex/config.toml`
//! (`[mcp_servers]`), OpenCode `opencode.json` of the folder and `~/.config/opencode/opencode.json`
//! (`mcp`). Those files hold secrets (env, headers, arguments, URL userinfo): only the server's
//! name, its transport and the file's path are ever picked out of them, nothing else is kept.
//!
//! `check_connectors` additionally asks `claude mcp list` / `codex mcp list` when
//! `Settings::external.mcp_live` is on (off by default; 10 s timeout, answers cached for 60 s).
//! **Unverified:** the output formats of both commands, shaped from what they print today; a line
//! that does not parse leaves the connector `unknown`. `claude mcp list` reports no tool count, so
//! `connected` carries `tools: 0`.

use crate::launch::{self, LaunchEnv};
use crate::state::{State, unavailable};
use lotse_proto::{
    AgentKind, Connector, ConnectorStatus, ErrorCode, ProtoError, Request, Response, SessionId,
};
use serde_json::Value;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, LazyLock, Mutex};
use std::time::{Duration, Instant};

const LIVE_TIMEOUT: Duration = Duration::from_secs(10);
const LIVE_CACHE: Duration = Duration::from_secs(60);
const LIVE_MAX_BYTES: u64 = 256 * 1024;
/// `~/.claude.json` grows with every project the user opened; a config beyond this is not read.
const MAX_CONFIG_BYTES: u64 = 16 * 1024 * 1024;
const UNREADABLE: &str = "unreadable config";

pub async fn handle(state: &Arc<State>, req: Request) -> Response {
    match req {
        Request::ListConnectors { session_id } => connectors(state, &session_id, false).await,
        Request::CheckConnectors { session_id } => connectors(state, &session_id, true).await,
        other => unavailable(other.method()),
    }
}

fn err(code: ErrorCode, message: impl Into<String>) -> Response {
    Response::Error(ProtoError {
        code,
        message: message.into(),
    })
}

async fn connectors(state: &Arc<State>, session_id: &SessionId, check: bool) -> Response {
    let Some(h) = state.session(session_id) else {
        return err(ErrorCode::NotFound, "session not found");
    };
    let cwd = h.cwd();
    let home = state.config().home.clone();
    let settings = state.settings();
    let live = check && settings.external.mcp_live;
    let env = if live {
        Some(state.launch_env().await)
    } else {
        None
    };
    let res = tokio::task::spawn_blocking(move || {
        let mut all = read_connectors(home.as_deref(), &cwd);
        if let Some(env) = env {
            apply_live(&mut all, &settings, &env, &cwd, home.as_deref());
        }
        all
    })
    .await;
    match res {
        Ok(all) => Response::Connectors(all),
        Err(e) => {
            tracing::error!("{e}");
            err(ErrorCode::Internal, "internal error")
        }
    }
}

// config files

fn read_connectors(home: Option<&Path>, cwd: &Path) -> Vec<Connector> {
    let mut out = Vec::new();
    let mut claude_project_keys = vec![cwd.to_string_lossy().into_owned()];
    if let Ok(real) = std::fs::canonicalize(cwd) {
        let real = real.to_string_lossy().into_owned();
        if !claude_project_keys.contains(&real) {
            claude_project_keys.push(real);
        }
    }
    let claude_user = |v: &Value| {
        let mut found = servers(&v["mcpServers"]);
        for key in &claude_project_keys {
            found.extend(servers(&v["projects"][key.as_str()]["mcpServers"]));
        }
        found
    };
    let opencode = |v: &Value| servers(&v["mcp"]);

    let mut source = |agent, path: PathBuf, pick: &dyn Fn(&Value) -> Vec<(String, String)>| {
        read_source(&mut out, agent, &path, |text| {
            serde_json::from_str::<Value>(text).ok().map(|v| pick(&v))
        });
    };
    source(AgentKind::ClaudeCode, cwd.join(".mcp.json"), &|v| {
        servers(&v["mcpServers"])
    });
    if let Some(home) = home {
        source(
            AgentKind::ClaudeCode,
            home.join(".claude.json"),
            &claude_user,
        );
    }
    source(AgentKind::OpenCode, cwd.join("opencode.json"), &opencode);
    if let Some(home) = home {
        source(
            AgentKind::OpenCode,
            home.join(".config/opencode/opencode.json"),
            &opencode,
        );
    }
    if let Some(home) = home {
        read_source(
            &mut out,
            AgentKind::Codex,
            &home.join(".codex/config.toml"),
            |text| {
                toml::from_str::<Value>(text)
                    .ok()
                    .map(|v| servers(&v["mcp_servers"]))
            },
        );
    }
    // stable: Claude Code, Codex, OpenCode, each in file order
    out.sort_by_key(|c| match c.agent {
        AgentKind::ClaudeCode => 0,
        AgentKind::Codex => 1,
        _ => 2,
    });
    out
}

/// One config file: a missing file contributes nothing, a file `parse` cannot read one failed row.
fn read_source(
    out: &mut Vec<Connector>,
    agent: AgentKind,
    path: &Path,
    parse: impl FnOnce(&str) -> Option<Vec<(String, String)>>,
) {
    let config_path = path.to_string_lossy().into_owned();
    let text = match std::fs::metadata(path) {
        Ok(m) if m.is_file() && m.len() <= MAX_CONFIG_BYTES => std::fs::read_to_string(path).ok(),
        Ok(m) if m.is_file() => None,
        Ok(_) => return,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return,
        Err(_) => None,
    };
    match text.as_deref().and_then(parse) {
        Some(found) => out.extend(found.into_iter().map(|(name, transport)| Connector {
            agent,
            name,
            transport,
            config_path: config_path.clone(),
            status: ConnectorStatus::Configured,
        })),
        None => out.push(Connector {
            agent,
            name: path
                .file_name()
                .map_or_else(|| config_path.clone(), |n| n.to_string_lossy().into_owned()),
            transport: "unknown".into(),
            config_path,
            status: ConnectorStatus::Failed {
                reason: UNREADABLE.into(),
            },
        }),
    }
}

/// `(name, transport)` of each server in an object of name → server config.
fn servers(v: &Value) -> Vec<(String, String)> {
    v.as_object().map_or_else(Vec::new, |m| {
        m.iter()
            .filter(|(_, cfg)| cfg.is_object())
            .map(|(name, cfg)| (name.clone(), transport_of(cfg)))
            .collect()
    })
}

/// Only fixed words come back, never a value out of the config.
fn transport_of(cfg: &Value) -> String {
    match cfg.get("type").and_then(Value::as_str) {
        Some("stdio" | "local") => "stdio",
        Some("sse") => "sse",
        Some("http" | "streamable-http" | "streamable_http" | "remote") => "http",
        _ if cfg.get("url").is_some() => "http",
        _ => "stdio",
    }
    .into()
}

// live status

type LiveMap = Option<HashMap<String, ConnectorStatus>>;
type CacheKey = (AgentKind, PathBuf, Option<PathBuf>);

static LIVE: LazyLock<Mutex<HashMap<CacheKey, (Instant, LiveMap)>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

/// Claude Code and Codex connectors take the status their CLI reports; one the CLI does not list,
/// or no answer at all (not installed, failed, timed out), is `unknown`. OpenCode stays `configured`.
fn apply_live(
    all: &mut [Connector],
    settings: &lotse_proto::Settings,
    env: &LaunchEnv,
    cwd: &Path,
    home: Option<&Path>,
) {
    let wanted = |agent| {
        all.iter()
            .any(|c| c.agent == agent && c.status == ConnectorStatus::Configured)
    };
    let agents: Vec<AgentKind> = [AgentKind::ClaudeCode, AgentKind::Codex]
        .into_iter()
        .filter(|&a| wanted(a))
        .collect();
    let answers: Vec<(AgentKind, LiveMap)> = std::thread::scope(|s| {
        let jobs: Vec<_> = agents
            .iter()
            .map(|&agent| {
                let command = launch::configured_command(settings, agent);
                s.spawn(move || (agent, live_status(agent, command, env, cwd, home)))
            })
            .collect();
        jobs.into_iter().filter_map(|j| j.join().ok()).collect()
    });
    for c in all
        .iter_mut()
        .filter(|c| c.status == ConnectorStatus::Configured)
    {
        if let Some((_, live)) = answers.iter().find(|(a, _)| *a == c.agent) {
            c.status = live
                .as_ref()
                .and_then(|m| m.get(&c.name).cloned())
                .unwrap_or(ConnectorStatus::Unknown);
        }
    }
}

fn live_status(
    agent: AgentKind,
    command: &str,
    env: &LaunchEnv,
    cwd: &Path,
    home: Option<&Path>,
) -> LiveMap {
    let key = (agent, cwd.to_path_buf(), home.map(Path::to_path_buf));
    if let Some((at, map)) = LIVE.lock().unwrap_or_else(|e| e.into_inner()).get(&key)
        && at.elapsed() < LIVE_CACHE
    {
        return map.clone();
    }
    let map = run_mcp_list(command, env, cwd).map(|out| match agent {
        AgentKind::Codex => parse_codex_list(&out),
        _ => parse_claude_list(&out),
    });
    LIVE.lock()
        .unwrap_or_else(|e| e.into_inner())
        .insert(key, (Instant::now(), map.clone()));
    map
}

/// `<program of command> mcp list` run in the session's folder (project-scoped servers).
fn run_mcp_list(command: &str, env: &LaunchEnv, cwd: &Path) -> Option<String> {
    let argv = env.tokenize(command).ok()?;
    let program = env.resolve(&argv, cwd)?;
    let (cwd, program) = (cwd.to_string_lossy(), program.to_string_lossy());
    // `sh -c` only to start in `cwd`: launch::run_capture has no working directory
    launch::run_capture(
        Path::new("/bin/sh"),
        &[
            "-c",
            "cd \"$1\" && exec \"$2\" mcp list",
            "sh",
            &cwd,
            &program,
        ],
        Some(env.vars()),
        LIVE_TIMEOUT,
        LIVE_MAX_BYTES,
    )
}

/// `name: <command or url> - ✓ Connected` / `✗ Failed to connect` / `⚠ Needs authentication`.
/// Only the name and the verdict are kept: the middle part can hold arguments with secrets.
fn parse_claude_list(out: &str) -> HashMap<String, ConnectorStatus> {
    out.lines()
        .filter_map(|line| {
            let (name, rest) = line.split_once(": ")?;
            let verdict = rest.rsplit_once(" - ")?.1.to_lowercase();
            let status = if verdict.contains("fail") {
                ConnectorStatus::Failed {
                    reason: "failed to connect".into(),
                }
            } else if verdict.contains("authenticat") {
                ConnectorStatus::NeedsAuth
            } else if verdict.contains("connected") {
                ConnectorStatus::Connected { tools: 0 }
            } else {
                return None;
            };
            Some((name.trim().to_owned(), status))
        })
        .collect()
}

/// A table: `Name Command Args Env Cwd Status Auth`, one server per row. `disabled` and
/// `Not logged in` are the verdicts; any other row of a known layout is `configured`.
fn parse_codex_list(out: &str) -> HashMap<String, ConnectorStatus> {
    let mut lines = out.lines();
    let has_header = lines
        .next()
        .is_some_and(|h| h.split_whitespace().next() == Some("Name"));
    if !has_header {
        return HashMap::new();
    }
    lines
        .filter_map(|line| {
            let name = line.split_whitespace().next()?;
            let row = line.to_lowercase();
            let status = if row.contains("disabled") {
                ConnectorStatus::Failed {
                    reason: "disabled".into(),
                }
            } else if row.contains("not logged in") {
                ConnectorStatus::NeedsAuth
            } else {
                ConnectorStatus::Configured
            };
            Some((name.to_owned(), status))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn claude_list_keeps_names_with_colons_and_drops_the_rest() {
        let m = parse_claude_list(
            "Checking MCP server health...\n\
             \n\
             plugin:a:b: npx -y x --token=SECRET - ✓ Connected\n\
             hub: https://h/mcp (HTTP) - ⚠ Needs authentication\n\
             bad: npx - ✗ Failed to connect\n\
             odd: npx - something else\n",
        );
        assert_eq!(m["plugin:a:b"], ConnectorStatus::Connected { tools: 0 });
        assert_eq!(m["hub"], ConnectorStatus::NeedsAuth);
        assert!(matches!(m["bad"], ConnectorStatus::Failed { .. }));
        assert_eq!(m.len(), 3, "{m:?}");
    }

    #[test]
    fn codex_list_needs_its_header() {
        assert!(parse_codex_list("no servers configured\n").is_empty());
        let m = parse_codex_list("Name  Status  Auth\na  enabled  OAuth\nb  disabled  -\n");
        assert_eq!(m["a"], ConnectorStatus::Configured);
        assert!(matches!(m["b"], ConnectorStatus::Failed { .. }));
    }

    #[test]
    fn transports_are_fixed_words() {
        let t = |s: &str| transport_of(&serde_json::from_str(s).unwrap());
        assert_eq!(t(r#"{"type":"sse","url":"u"}"#), "sse");
        assert_eq!(t(r#"{"type":"remote","url":"u"}"#), "http");
        assert_eq!(t(r#"{"url":"https://u:SECRET@h"}"#), "http");
        assert_eq!(t(r#"{"command":"x"}"#), "stdio");
        assert_eq!(t(r#"{"type":"SECRET-TYPE"}"#), "stdio");
    }
}
