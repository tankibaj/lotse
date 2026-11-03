//! MCP connectors on the real daemon: `list_connectors`, `check_connectors`. Temp HOME with configs
//! that hold fake secrets (every one contains `SECRET`): none may ever come back. The agents' CLIs
//! are fake scripts, never the real ones.

mod common;

use common::{Client, Daemon};
use lotse_proto::{
    AgentKind, Connector, ConnectorStatus, ErrorCode, NewSession, Project, Request, Response,
    SessionId,
};
use std::path::{Path, PathBuf};

struct Fixture {
    d: Daemon,
    c: Client,
    project: Project,
    session_id: SessionId,
}

fn start_with(extra_path: Option<&Path>) -> Fixture {
    let d = Daemon::start_with_path(extra_path);
    let mut c = d.client();
    let project = c.add_project(&d.project_dir("praxis-web"));
    let session = match c.request(Request::CreateSession(NewSession {
        project_id: project.id.clone(),
        agent: AgentKind::Terminal,
        command: Some("/bin/sh".into()),
        title: None,
        worktree: None,
        first_prompt: None,
    })) {
        Response::Session(s) => s,
        other => panic!("create_session: {other:?}"),
    };
    Fixture {
        d,
        c,
        project,
        session_id: session.id,
    }
}

fn start() -> Fixture {
    start_with(None)
}

impl Fixture {
    fn project_path(&self) -> PathBuf {
        PathBuf::from(&self.project.path)
    }

    fn home(&self, rel: &str) -> PathBuf {
        let p = self.d.home.join(rel);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        p
    }

    fn list(&mut self) -> Vec<Connector> {
        match self.c.request(Request::ListConnectors {
            session_id: self.session_id.clone(),
        }) {
            Response::Connectors(v) => v,
            other => panic!("list_connectors: {other:?}"),
        }
    }

    fn check(&mut self) -> Vec<Connector> {
        match self.c.request(Request::CheckConnectors {
            session_id: self.session_id.clone(),
        }) {
            Response::Connectors(v) => v,
            other => panic!("check_connectors: {other:?}"),
        }
    }

    fn enable_live(&mut self) {
        let Response::Settings(mut s) = self.c.request(Request::GetSettings) else {
            panic!("get_settings")
        };
        s.external.mcp_live = true;
        assert!(matches!(
            self.c.request(Request::UpdateSettings { settings: s }),
            Response::Settings(_)
        ));
    }
}

/// Every agent's config, with a secret in each env, header, argument and URL.
fn write_all_configs(fx: &Fixture) {
    std::fs::write(
        fx.project_path().join(".mcp.json"),
        r#"{"mcpServers":{
            "github":{"type":"http","url":"https://user:SECRET-USERINFO@api.example.com/mcp",
                      "headers":{"Authorization":"Bearer SECRET-HEADER"}},
            "local-db":{"command":"npx","args":["-y","db","--password=SECRET-ARG"],
                        "env":{"DB_PASSWORD":"SECRET-ENV"}}}}"#,
    )
    .unwrap();
    let project_key = fx.project.path.clone();
    std::fs::write(
        fx.home(".claude.json"),
        format!(
            r#"{{"mcpServers":{{"user-srv":{{"command":"x","env":{{"K":"SECRET-USER"}}}}}},
                "projects":{{"{project_key}":{{"mcpServers":{{"proj-srv":{{"type":"sse","url":"https://h/sse"}}}}}},
                             "/somewhere/else":{{"mcpServers":{{"other-project":{{"command":"y"}}}}}}}}}}"#
        ),
    )
    .unwrap();
    std::fs::write(
        fx.home(".codex/config.toml"),
        "model = \"gpt-5\"\n\
         [mcp_servers.docs]\ncommand = \"npx\"\nargs = [\"-y\", \"docs\", \"--key=SECRET-CODEX-ARG\"]\n\
         [mcp_servers.docs.env]\nTOKEN = \"SECRET-CODEX-ENV\"\n\
         [mcp_servers.remote]\nurl = \"https://example.com/mcp\"\nbearer_token_env_var = \"SECRET-CODEX-VAR\"\n",
    )
    .unwrap();
    std::fs::write(
        fx.project_path().join("opencode.json"),
        r#"{"mcp":{"oc-local":{"type":"local","command":["npx","-y","x"],"environment":{"K":"SECRET-OC-ENV"}}}}"#,
    )
    .unwrap();
    std::fs::write(
        fx.home(".config/opencode/opencode.json"),
        r#"{"mcp":{"oc-remote":{"type":"remote","url":"https://h/mcp","headers":{"Authorization":"SECRET-OC-HEADER"}}}}"#,
    )
    .unwrap();
}

fn find<'a>(all: &'a [Connector], agent: AgentKind, name: &str) -> &'a Connector {
    all.iter()
        .find(|c| c.agent == agent && c.name == name)
        .unwrap_or_else(|| panic!("no {agent:?} connector {name}: {all:?}"))
}

fn error_code(r: &Response) -> ErrorCode {
    match r {
        Response::Error(e) => e.code,
        other => panic!("expected an error, got {other:?}"),
    }
}

#[test]
fn connectors_hide_env_values() {
    let mut fx = start();
    write_all_configs(&fx);
    for all in [fx.list(), fx.check()] {
        let wire = serde_json::to_string(&Response::Connectors(all)).unwrap();
        assert!(!wire.contains("SECRET"), "a secret leaked: {wire}");
        assert!(!wire.contains("Bearer"), "{wire}");
    }
}

#[test]
fn connectors_per_agent() {
    let mut fx = start();
    write_all_configs(&fx);
    let all = fx.list();
    let project = fx.project_path();
    let mcp_json = project.join(".mcp.json").to_string_lossy().into_owned();
    let claude_json =
        fx.d.home
            .join(".claude.json")
            .to_string_lossy()
            .into_owned();
    let codex_toml =
        fx.d.home
            .join(".codex/config.toml")
            .to_string_lossy()
            .into_owned();

    let expect = [
        (AgentKind::ClaudeCode, "github", "http", &mcp_json),
        (AgentKind::ClaudeCode, "local-db", "stdio", &mcp_json),
        (AgentKind::ClaudeCode, "user-srv", "stdio", &claude_json),
        (AgentKind::ClaudeCode, "proj-srv", "sse", &claude_json),
        (AgentKind::Codex, "docs", "stdio", &codex_toml),
        (AgentKind::Codex, "remote", "http", &codex_toml),
    ];
    for (agent, name, transport, path) in expect {
        let c = find(&all, agent, name);
        assert_eq!(c.transport, transport, "{c:?}");
        assert_eq!(&c.config_path, path, "{c:?}");
        assert_eq!(c.status, ConnectorStatus::Configured, "{c:?}");
    }
    let oc_local = find(&all, AgentKind::OpenCode, "oc-local");
    assert_eq!(oc_local.transport, "stdio");
    assert!(oc_local.config_path.ends_with("praxis-web/opencode.json"));
    let oc_remote = find(&all, AgentKind::OpenCode, "oc-remote");
    assert_eq!(oc_remote.transport, "http");
    assert!(
        oc_remote
            .config_path
            .ends_with(".config/opencode/opencode.json")
    );
    // another project's servers in `~/.claude.json` are not this session's
    assert!(all.iter().all(|c| c.name != "other-project"), "{all:?}");
    assert_eq!(all.len(), 8, "{all:?}");
}

#[test]
fn no_mcp_configured_lists_nothing() {
    let mut fx = start();
    assert_eq!(fx.list(), Vec::<Connector>::new());
    assert_eq!(fx.check(), Vec::<Connector>::new());
    // a config without the key is not an error either
    std::fs::write(fx.home(".claude.json"), r#"{"numStartups":3}"#).unwrap();
    std::fs::write(fx.home(".codex/config.toml"), "model = \"gpt-5\"\n").unwrap();
    assert_eq!(fx.list(), Vec::<Connector>::new());
}

#[test]
fn a_malformed_config_fails_that_source_only() {
    let mut fx = start();
    write_all_configs(&fx);
    std::fs::write(
        fx.project_path().join(".mcp.json"),
        "{ not json SECRET-GARBAGE",
    )
    .unwrap();
    std::fs::write(
        fx.home(".codex/config.toml"),
        "[mcp_servers.docs\nSECRET = ",
    )
    .unwrap();
    let all = fx.list();
    for (agent, path) in [
        (AgentKind::ClaudeCode, fx.project_path().join(".mcp.json")),
        (AgentKind::Codex, fx.d.home.join(".codex/config.toml")),
    ] {
        let failed: Vec<_> = all
            .iter()
            .filter(|c| c.agent == agent && c.config_path == path.to_string_lossy())
            .collect();
        assert_eq!(failed.len(), 1, "{all:?}");
        assert_eq!(
            failed[0].status,
            ConnectorStatus::Failed {
                reason: "unreadable config".into()
            }
        );
    }
    // the other sources are still listed
    find(&all, AgentKind::ClaudeCode, "user-srv");
    find(&all, AgentKind::ClaudeCode, "proj-srv");
    find(&all, AgentKind::OpenCode, "oc-local");
    find(&all, AgentKind::OpenCode, "oc-remote");
    let wire = serde_json::to_string(&Response::Connectors(all)).unwrap();
    assert!(!wire.contains("SECRET"), "{wire}");
}

#[test]
fn unknown_session_is_not_found() {
    let d = Daemon::start();
    let mut c = d.client();
    for req in [
        Request::ListConnectors {
            session_id: SessionId::new("nope"),
        },
        Request::CheckConnectors {
            session_id: SessionId::new("nope"),
        },
    ] {
        let r = c.request(req);
        assert_eq!(error_code(&r), ErrorCode::NotFound, "{r:?}");
    }
}

/// A directory with fake `claude` and `codex` that record being run and print `claude_out` /
/// `codex_out`.
fn fake_clis(claude_out: &str, codex_out: &str) -> (tempfile::TempDir, PathBuf) {
    let bin = tempfile::tempdir().unwrap();
    let marker = bin.path().join("ran");
    let script = |out: &str| {
        format!(
            "[ \"$1 $2\" = \"mcp list\" ] || exit 1\n\
             echo \"$0\" >> '{}'\n\
             cat <<'EOF'\n{out}\nEOF",
            marker.display()
        )
    };
    common::script(bin.path(), "claude", &script(claude_out));
    common::script(bin.path(), "codex", &script(codex_out));
    (bin, marker)
}

const CLAUDE_LIST: &str = "Checking MCP server health...\n\
\n\
github: https://user:SECRET-LIVE@api.example.com/mcp (HTTP) - ✗ Failed to connect\n\
local-db: npx -y db --password=SECRET-LIVE-ARG - ✓ Connected\n\
proj-srv: https://h/sse (SSE) - ⚠ Needs authentication\n\
plugin:other:thing: npx -y z - ✓ Connected";

const CODEX_LIST: &str = "Name    Command  Args  Env  Cwd  Status    Auth\n\
docs    npx      -y    -    -    enabled   Unsupported\n\
remote  -        -     -    -    enabled   Not logged in";

#[test]
fn live_check_disabled_by_default() {
    let (bin, marker) = fake_clis(CLAUDE_LIST, CODEX_LIST);
    let mut fx = start_with(Some(bin.path()));
    write_all_configs(&fx);
    let listed = fx.list();
    assert_eq!(fx.check(), listed, "check_connectors without mcp_live");
    assert!(
        !marker.exists(),
        "an agent CLI ran with settings.external.mcp_live off"
    );
}

#[test]
fn live_check_reads_mcp_list_when_enabled() {
    let (bin, marker) = fake_clis(CLAUDE_LIST, CODEX_LIST);
    let mut fx = start_with(Some(bin.path()));
    write_all_configs(&fx);
    fx.enable_live();
    let all = fx.check();
    assert!(marker.exists(), "no agent CLI ran");

    let status = |agent, name: &str| find(&all, agent, name).status.clone();
    assert!(matches!(
        status(AgentKind::ClaudeCode, "github"),
        ConnectorStatus::Failed { .. }
    ));
    assert!(matches!(
        status(AgentKind::ClaudeCode, "local-db"),
        ConnectorStatus::Connected { .. }
    ));
    assert_eq!(
        status(AgentKind::ClaudeCode, "proj-srv"),
        ConnectorStatus::NeedsAuth
    );
    // configured but not in the CLI's answer
    assert_eq!(
        status(AgentKind::ClaudeCode, "user-srv"),
        ConnectorStatus::Unknown
    );
    assert_eq!(
        status(AgentKind::Codex, "docs"),
        ConnectorStatus::Configured
    );
    assert_eq!(
        status(AgentKind::Codex, "remote"),
        ConnectorStatus::NeedsAuth
    );
    // OpenCode has no live check
    assert_eq!(
        status(AgentKind::OpenCode, "oc-local"),
        ConnectorStatus::Configured
    );
    // a server only the CLI knows is not invented
    assert!(
        all.iter().all(|c| c.name != "plugin:other:thing"),
        "{all:?}"
    );
    let wire = serde_json::to_string(&Response::Connectors(all)).unwrap();
    assert!(!wire.contains("SECRET"), "a secret leaked: {wire}");
}

#[test]
fn live_check_without_the_cli_is_unknown() {
    // PATH holds no `claude` or `codex`
    let mut fx = start();
    write_all_configs(&fx);
    fx.enable_live();
    let all = fx.check();
    assert_eq!(
        find(&all, AgentKind::ClaudeCode, "github").status,
        ConnectorStatus::Unknown
    );
    assert_eq!(
        find(&all, AgentKind::Codex, "docs").status,
        ConnectorStatus::Unknown
    );
    assert_eq!(
        find(&all, AgentKind::OpenCode, "oc-local").status,
        ConnectorStatus::Configured
    );
}
