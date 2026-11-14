//! What a pane's environment says about coding agents.
//!
//! * [`scrub_host_markers`]: Lotse may itself have been started from inside an agent (Claude Code's
//!   Bash tool, a Codex session, …). The markers that agent exports to its children would make the
//!   agent in a Lotse pane believe it is a child session, so they are dropped. User configuration
//!   (`ANTHROPIC_*`, `CODEX_HOME`, `CLAUDE_CONFIG_DIR`, `CLAUDE_CODE_USE_BEDROCK`, …) stays.
//! * [`Injection`]: status hooks that need no global install. Claude Code gets the Lotse plugin
//!   through `--plugin-dir` on the launch command (and `CLAUDE_CODE_PLUGIN_DIRS` for older Claude: a
//!   settings.json `env` overrides the process variable), OpenCode gets the Lotse plugin through an overlay
//!   `OPENCODE_CONFIG_DIR`, Codex gets a mirrored `CODEX_HOME` (its hooks need a trusted entry in
//!   a `config.toml`, see `integrations/codex/README.md`); all per pane, nothing in `~/.claude`,
//!   `~/.config/opencode` or `~/.codex` changes.

use lotse_hook::{claude_plugin, codex_home, mcp_inject, opencode_plugin};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// Variables an agent exports to what it runs, by exact name. Claude Code: its own list of ambient
/// markers (read from the 2.1.309 binary) plus the ones Orca strips
/// (`src/main/ipc/pty/host-env/spawn-env-keys.ts`); Codex and OpenCode: read from the installed
/// binaries (Codex 0.162.1, OpenCode 1.19.4), Orca scrubs neither.
const HOST_MARKERS: &[&str] = &[
    // Claude Code
    "CLAUDECODE",
    "CLAUDE_PID",
    "CLAUDE_EFFORT",
    "CLAUDE_CODE_ENTRYPOINT",
    // bare mode (`claude --bare`): hooks, plugins and auto-memory off. Inherited from a parent
    // session it would silence every Lotse hook in the pane's own Claude.
    "CLAUDE_CODE_SIMPLE",
    "CLAUDE_CODE_EXECPATH",
    "CLAUDE_CODE_SESSION_ID",
    "CLAUDE_CODE_CHILD_SESSION",
    "CLAUDE_CODE_BRIDGE_SESSION_ID",
    "CLAUDE_CODE_SESSION_ATTENDED",
    "CLAUDE_CODE_SESSION_LOG",
    "CLAUDE_CODE_AGENT",
    "CLAUDE_CODE_HOST_WORKTREE",
    "CLAUDE_CODE_HOST_WORKTREE_FENCE",
    "CLAUDE_CODE_HOST_AUTH_ENV_VAR",
    // Codex: the sandbox and thread it runs commands in
    "CODEX_THREAD_ID",
    "CODEX_SANDBOX",
    "CODEX_SANDBOX_NETWORK_DISABLED",
    "CODEX_NETWORK_PROXY_ACTIVE",
    "CODEX_ESCALATE_SOCKET",
    "CODEX_INTERNAL_ORIGINATOR_OVERRIDE",
    // OpenCode
    "OPENCODE",
    "OPENCODE_PID",
    "OPENCODE_TERMINAL",
];

/// Families of host-set variables. `CLAUDE_CODE_*` as a whole is not here on purpose: it also holds
/// user configuration (`CLAUDE_CODE_USE_BEDROCK`, `CLAUDE_CODE_OAUTH_TOKEN`, `CLAUDE_CODE_PLUGIN_DIRS`, …).
const HOST_MARKER_PREFIXES: &[&str] = &[
    "CLAUDE_AGENT_SDK_",
    "CLAUDE_PREVIEW_",
    "CLAUDE_CODE_MESSAGING_",
    "CODEX_SNAPSHOT_",
];

/// Drop the "I am running inside an agent" variables from `vars`.
pub fn scrub_host_markers(vars: &mut HashMap<String, String>) {
    vars.retain(|name, _| {
        !HOST_MARKERS.contains(&name.as_str())
            && !HOST_MARKER_PREFIXES.iter().any(|p| name.starts_with(p))
    });
}

const CLAUDE_PLUGIN_DIRS: &str = "CLAUDE_CODE_PLUGIN_DIRS";
const OPENCODE_CONFIG_DIR: &str = "OPENCODE_CONFIG_DIR";
const OPENCODE_CONFIG_CONTENT: &str = "OPENCODE_CONFIG_CONTENT";
const CODEX_HOME: &str = "CODEX_HOME";
/// The user's own Codex home, next to the mirror in `CODEX_HOME`: a pane started from a Lotse pane
/// inherits both and must still mirror the user's home, not the mirror.
const CODEX_SOURCE_HOME: &str = "LOTSE_CODEX_SOURCE_HOME";
/// `LOTSE_CODEX_HOOKS=off` in the login environment turns the Codex mirror off.
const CODEX_HOOKS_SWITCH: &str = "LOTSE_CODEX_HOOKS";

/// Plugins written at daemon start, and how they reach a pane.
#[derive(Debug, Clone, Default)]
pub struct Injection {
    claude_plugin: Option<PathBuf>,
    opencode_overlays: Option<PathBuf>,
    codex_mirrors: Option<PathBuf>,
}

impl Injection {
    /// Write the plugins under `<data dir>/integrations/`. A failure is logged and that agent simply
    /// gets no injection (the global install and the screen fallback still work).
    pub fn prepare(data_dir: &Path) -> Self {
        // Claude Code only accepts absolute plugin paths.
        let base = std::path::absolute(data_dir)
            .unwrap_or_else(|_| data_dir.to_owned())
            .join("integrations");
        let claude_plugin = match claude_plugin::ensure(&base.join("claude")) {
            Ok(dir) => {
                // child sessions: the `lotse` MCP server rides in the same plugin
                if let Err(e) = mcp_inject::ensure_claude(&dir) {
                    tracing::warn!("Claude Code gets no lotse MCP server: {e:#}");
                }
                Some(dir)
            }
            Err(e) => {
                tracing::warn!(
                    "Claude Code plugin not written, panes get no injected hooks: {e:#}"
                );
                None
            }
        };
        // The overlays are made per pane; the default one now proves the directory is writable.
        let opencode = base.join("opencode");
        let opencode_overlays = match opencode_plugin::overlay(&opencode, None) {
            Ok(_) => Some(opencode),
            Err(e) => {
                tracing::warn!("OpenCode plugin not written, panes get no injected plugin: {e:#}");
                None
            }
        };
        // The mirrors are made per pane, from the user's home; here only the directory is checked.
        let codex = base.join("codex");
        let codex_mirrors = match std::fs::create_dir_all(&codex) {
            Ok(()) => Some(codex),
            Err(e) => {
                tracing::warn!("Codex home mirror not possible, panes get no injected hooks: {e}");
                None
            }
        };
        Self {
            claude_plugin,
            opencode_overlays,
            codex_mirrors,
        }
    }

    /// The Lotse plugin directory, for `--plugin-dir` on a Claude Code launch command (see
    /// [`crate::launch::add_claude_plugin_dir`]); `None` when it could not be written.
    pub fn claude_plugin_dir(&self) -> Option<&Path> {
        self.claude_plugin.as_deref()
    }

    /// Put the injection into a pane's environment.
    pub fn apply(&self, vars: &mut HashMap<String, String>) {
        // the daemon exports it before calling this; Codex and OpenCode do not expand variables
        let hook_bin = vars
            .get("LOTSE_HOOK_BIN")
            .filter(|b| !b.is_empty())
            .cloned()
            .unwrap_or_else(|| "lotse-hook".to_owned());
        if let Some(plugin) = &self.claude_plugin {
            let mut dirs: Vec<PathBuf> = vars
                .get(CLAUDE_PLUGIN_DIRS)
                .map(|v| {
                    std::env::split_paths(v)
                        .filter(|p| !p.as_os_str().is_empty())
                        .collect()
                })
                .unwrap_or_default();
            if !dirs.contains(plugin) {
                dirs.push(plugin.clone());
            }
            match std::env::join_paths(&dirs) {
                Ok(joined) => {
                    vars.insert(
                        CLAUDE_PLUGIN_DIRS.into(),
                        joined.to_string_lossy().into_owned(),
                    );
                }
                Err(e) => tracing::warn!("{CLAUDE_PLUGIN_DIRS} left alone: {e}"),
            }
        }
        if let Some(root) = &self.opencode_overlays {
            // A user's own directory is mirrored; an inherited overlay of ours is not (a Lotse pane
            // started from a Lotse pane).
            let source = vars
                .get(OPENCODE_CONFIG_DIR)
                .filter(|d| !d.is_empty())
                .map(PathBuf::from)
                .filter(|d| !d.starts_with(root));
            match opencode_plugin::overlay(root, source.as_deref()) {
                Ok(dir) => {
                    vars.insert(
                        OPENCODE_CONFIG_DIR.into(),
                        dir.to_string_lossy().into_owned(),
                    );
                }
                Err(e) => tracing::warn!("{OPENCODE_CONFIG_DIR} left alone: {e:#}"),
            }
            // the overlay links the user's opencode.json: the MCP server goes in the inline config
            let inline = vars.get(OPENCODE_CONFIG_CONTENT).map(String::as_str);
            if let Some(content) = mcp_inject::opencode_config_content(inline, &hook_bin) {
                vars.insert(OPENCODE_CONFIG_CONTENT.into(), content);
            }
        }
        let codex_off = vars.get(CODEX_HOOKS_SWITCH).is_some_and(|v| {
            matches!(
                v.trim().to_lowercase().as_str(),
                "0" | "off" | "false" | "no"
            )
        });
        if let Some(root) = self.codex_mirrors.as_ref().filter(|_| !codex_off)
            && let Some(source) = codex_source(vars, root)
        {
            match codex_home::mirror(root, &source) {
                Ok(dir) => {
                    match mcp_inject::ensure_codex(&dir.join("config.toml"), &hook_bin) {
                        Ok(mcp_inject::CodexOutcome::Skipped) => {
                            tracing::info!(
                                "Codex config has its own `lotse` MCP server: not added"
                            );
                        }
                        Ok(_) => {}
                        Err(e) => tracing::warn!("Codex gets no lotse MCP server: {e:#}"),
                    }
                    vars.insert(CODEX_HOME.into(), dir.to_string_lossy().into_owned());
                    vars.insert(
                        CODEX_SOURCE_HOME.into(),
                        source.to_string_lossy().into_owned(),
                    );
                }
                Err(e) => tracing::warn!("{CODEX_HOME} left alone: {e:#}"),
            }
        }
    }
}

/// The user's Codex home for a pane: their `CODEX_HOME`, else `$HOME/.codex`; `None` when it is not
/// there (Codex was never run: the first pane after it was gets the hooks). `HOME` is the pane's
/// (the login environment's), never guessed. Our own mirror in `CODEX_HOME`, inherited from a Lotse
/// pane, is replaced by the source it was made from.
fn codex_source(vars: &HashMap<String, String>, root: &Path) -> Option<PathBuf> {
    let set = |name: &str| {
        vars.get(name)
            .filter(|v| !v.is_empty())
            .map(PathBuf::from)
            .filter(|p| p.is_absolute())
    };
    let default = || set("HOME").map(|home| home.join(".codex"));
    let source = match set(CODEX_HOME) {
        Some(home) if !home.starts_with(root) => Some(home),
        Some(_) => set(CODEX_SOURCE_HOME)
            .filter(|d| !d.starts_with(root))
            .or_else(default),
        None if vars.get(CODEX_HOME).is_some_and(|v| !v.is_empty()) => None, // relative
        None => default(),
    };
    source.filter(|dir| dir.is_dir())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn map(pairs: &[(&str, &str)]) -> HashMap<String, String> {
        pairs
            .iter()
            .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
            .collect()
    }

    #[test]
    fn scrub_drops_inherited_agent_host_markers_and_keeps_user_config() {
        let mut vars = map(&[
            // markers of a parent Claude Code session
            ("CLAUDECODE", "1"),
            ("CLAUDE_CODE_CHILD_SESSION", "1"),
            ("CLAUDE_CODE_SESSION_ID", "abc"),
            ("CLAUDE_CODE_ENTRYPOINT", "cli"),
            // "bare" mode of the parent: no hooks, no plugins: Lotse's would never report
            ("CLAUDE_CODE_SIMPLE", "1"),
            ("CLAUDE_CODE_MESSAGING_SOCKET", "/tmp/x.sock"),
            ("CLAUDE_CODE_MESSAGING_TOKEN", "t"),
            ("CLAUDE_PID", "42"),
            ("CLAUDE_EFFORT", "high"),
            ("CLAUDE_AGENT_SDK_VERSION", "0.2"),
            ("CLAUDE_PREVIEW_PORT", "3000"),
            // Codex and OpenCode parents
            ("CODEX_THREAD_ID", "t1"),
            ("CODEX_SANDBOX", "seatbelt"),
            ("CODEX_SANDBOX_NETWORK_DISABLED", "1"),
            ("CODEX_SNAPSHOT_ORIGINAL_ENV", "x"),
            ("OPENCODE", "1"),
            ("OPENCODE_PID", "7"),
            ("OPENCODE_TERMINAL", "1"),
            // the user's own configuration
            ("ANTHROPIC_API_KEY", "k"),
            ("ANTHROPIC_BASE_URL", "https://example.test"),
            ("CLAUDE_CONFIG_DIR", "/home/u/.claude-work"),
            ("CLAUDE_CODE_USE_BEDROCK", "1"),
            ("CLAUDE_CODE_OAUTH_TOKEN", "o"),
            ("CLAUDE_CODE_PLUGIN_DIRS", "/user/plugin"),
            ("CODEX_HOME", "/home/u/.codex-work"),
            ("CODEX_API_KEY", "c"),
            ("OPENCODE_CONFIG_DIR", "/home/u/oc"),
            ("OPENCODE_SERVER_PASSWORD", "p"),
            ("PATH", "/usr/bin"),
            ("HOME", "/home/u"),
        ]);
        scrub_host_markers(&mut vars);

        let mut kept: Vec<_> = vars.keys().map(String::as_str).collect();
        kept.sort_unstable();
        assert_eq!(
            kept,
            [
                "ANTHROPIC_API_KEY",
                "ANTHROPIC_BASE_URL",
                "CLAUDE_CODE_OAUTH_TOKEN",
                "CLAUDE_CODE_PLUGIN_DIRS",
                "CLAUDE_CODE_USE_BEDROCK",
                "CLAUDE_CONFIG_DIR",
                "CODEX_API_KEY",
                "CODEX_HOME",
                "HOME",
                "OPENCODE_CONFIG_DIR",
                "OPENCODE_SERVER_PASSWORD",
                "PATH",
            ]
        );
    }

    #[test]
    fn claude_plugin_dir_is_appended_once_after_the_users_entries() {
        let tmp = tempfile::tempdir().unwrap();
        let inj = Injection::prepare(tmp.path());
        let plugin = tmp.path().join("integrations/claude/lotse-status");
        assert!(plugin.join("hooks/hooks.json").is_file());
        assert!(plugin.join(".claude-plugin/plugin.json").is_file());

        let mut vars = map(&[("CLAUDE_CODE_PLUGIN_DIRS", "/user/a::/user/b")]);
        inj.apply(&mut vars);
        assert_eq!(
            vars["CLAUDE_CODE_PLUGIN_DIRS"],
            format!("/user/a:/user/b:{}", plugin.display())
        );
        // a pane started from a Lotse pane inherits our entry already: no duplicate
        let once = vars.clone();
        inj.apply(&mut vars);
        assert_eq!(vars, once);

        let mut bare = HashMap::new();
        inj.apply(&mut bare);
        assert_eq!(
            bare["CLAUDE_CODE_PLUGIN_DIRS"],
            plugin.display().to_string()
        );
    }

    #[test]
    fn the_claude_plugin_dir_is_exposed_for_the_launch_command() {
        let tmp = tempfile::tempdir().unwrap();
        let inj = Injection::prepare(tmp.path());
        assert_eq!(
            inj.claude_plugin_dir(),
            Some(
                tmp.path()
                    .join("integrations/claude/lotse-status")
                    .as_path()
            )
        );
        let broken = Injection::prepare(Path::new("/proc/definitely/not/writable"));
        assert_eq!(broken.claude_plugin_dir(), None);
    }

    #[test]
    fn opencode_gets_an_overlay_and_a_users_config_dir_is_mirrored_not_replaced() {
        let tmp = tempfile::tempdir().unwrap();
        let inj = Injection::prepare(tmp.path());

        let mut vars = HashMap::new();
        inj.apply(&mut vars);
        let default = PathBuf::from(&vars["OPENCODE_CONFIG_DIR"]);
        assert!(default.starts_with(tmp.path()));
        assert!(default.join("plugins/lotse-status.js").is_file());

        // the user's own directory keeps working through the overlay
        let user = tmp.path().join("user-oc");
        std::fs::create_dir_all(&user).unwrap();
        std::fs::write(user.join("opencode.json"), "{}").unwrap();
        let mut vars = map(&[("OPENCODE_CONFIG_DIR", user.to_str().unwrap())]);
        inj.apply(&mut vars);
        let overlay = PathBuf::from(&vars["OPENCODE_CONFIG_DIR"]);
        assert_ne!(overlay, user);
        assert!(overlay.join("opencode.json").exists());
        assert!(overlay.join("plugins/lotse-status.js").is_file());
        assert!(
            !user.join("plugins").exists(),
            "the user's directory is untouched"
        );

        // a pane started from a Lotse pane inherits our overlay: it is not mirrored again
        let mut again = map(&[("OPENCODE_CONFIG_DIR", overlay.to_str().unwrap())]);
        inj.apply(&mut again);
        assert_eq!(PathBuf::from(&again["OPENCODE_CONFIG_DIR"]), default);
    }

    /// A user's Codex home with a login, and a data dir for the injection.
    struct CodexSetup {
        tmp: tempfile::TempDir,
        inj: Injection,
        home: PathBuf,
    }

    fn codex_setup() -> CodexSetup {
        let tmp = tempfile::tempdir().unwrap();
        let home = tmp.path().join("user");
        std::fs::create_dir_all(home.join(".codex")).unwrap();
        std::fs::write(home.join(".codex/auth.json"), "{\"token\":\"t\"}").unwrap();
        let inj = Injection::prepare(&tmp.path().join("data"));
        CodexSetup { tmp, inj, home }
    }

    #[test]
    fn codex_gets_a_mirror_of_the_users_home_with_the_hooks_and_the_users_home_is_untouched() {
        let s = codex_setup();
        let mut vars = map(&[("HOME", s.home.to_str().unwrap())]);
        s.inj.apply(&mut vars);

        let mirror = PathBuf::from(&vars["CODEX_HOME"]);
        assert!(mirror.starts_with(s.tmp.path().join("data/integrations/codex")));
        assert_eq!(
            vars["LOTSE_CODEX_SOURCE_HOME"],
            s.home.join(".codex").to_str().unwrap()
        );
        // the login is the real file's, reached through the mirror
        assert_eq!(
            std::fs::read_to_string(mirror.join("auth.json")).unwrap(),
            "{\"token\":\"t\"}"
        );
        let hooks: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(mirror.join("hooks.json")).unwrap())
                .unwrap();
        for event in [
            "SessionStart",
            "PermissionRequest",
            "SubagentStart",
            "SubagentStop",
            "Stop",
        ] {
            assert_eq!(
                hooks["hooks"][event][0]["hooks"][0]["command"],
                lotse_hook::codex_home::HOOK_COMMAND,
                "{event}"
            );
        }
        assert!(
            std::fs::read_to_string(mirror.join("config.toml"))
                .unwrap()
                .contains("trusted_hash")
        );
        // nothing was added to the user's home
        let mut left: Vec<_> = std::fs::read_dir(s.home.join(".codex"))
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        left.sort();
        assert_eq!(left, ["auth.json"]);
    }

    #[test]
    fn a_users_codex_home_variable_is_mirrored_and_an_inherited_mirror_is_not_mirrored_again() {
        let s = codex_setup();
        let work = s.tmp.path().join("work-codex");
        std::fs::create_dir_all(&work).unwrap();
        std::fs::write(work.join("config.toml"), "model = \"work\"\n").unwrap();

        let mut vars = map(&[
            ("HOME", s.home.to_str().unwrap()),
            ("CODEX_HOME", work.to_str().unwrap()),
        ]);
        s.inj.apply(&mut vars);
        let mirror = PathBuf::from(&vars["CODEX_HOME"]);
        assert_ne!(mirror, work);
        assert_eq!(vars["LOTSE_CODEX_SOURCE_HOME"], work.to_str().unwrap());
        assert!(
            std::fs::read_to_string(mirror.join("config.toml"))
                .unwrap()
                .contains("model = \"work\"")
        );

        // a pane started from a Lotse pane inherits both variables: same mirror, still the user's source
        let mut nested = vars.clone();
        s.inj.apply(&mut nested);
        assert_eq!(nested, vars);
        // and with only our CODEX_HOME left, the default home is used
        let mut lost = map(&[
            ("HOME", s.home.to_str().unwrap()),
            ("CODEX_HOME", mirror.to_str().unwrap()),
        ]);
        s.inj.apply(&mut lost);
        assert_eq!(
            lost["LOTSE_CODEX_SOURCE_HOME"],
            s.home.join(".codex").to_str().unwrap()
        );
    }

    #[test]
    fn codex_is_left_alone_without_a_home_to_mirror_or_with_one_that_cannot_be_mirrored() {
        let s = codex_setup();
        // no HOME: the real home is never guessed
        let mut vars = HashMap::new();
        s.inj.apply(&mut vars);
        assert!(!vars.contains_key("CODEX_HOME"));
        // the user never ran Codex: no ~/.codex to mirror
        let fresh = s.tmp.path().join("fresh");
        std::fs::create_dir_all(&fresh).unwrap();
        let mut vars = map(&[("HOME", fresh.to_str().unwrap())]);
        s.inj.apply(&mut vars);
        assert!(!vars.contains_key("CODEX_HOME"));
        // a relative CODEX_HOME means nothing outside the pane's directory
        let mut vars = map(&[("HOME", s.home.to_str().unwrap()), ("CODEX_HOME", ".codex")]);
        s.inj.apply(&mut vars);
        assert_eq!(vars["CODEX_HOME"], ".codex");
        // a config.toml the trust entries cannot be added to: the user's variable stays
        let broken = s.tmp.path().join("broken");
        std::fs::create_dir_all(&broken).unwrap();
        std::fs::write(broken.join("config.toml"), "model = ").unwrap();
        let mut vars = map(&[
            ("HOME", s.home.to_str().unwrap()),
            ("CODEX_HOME", broken.to_str().unwrap()),
        ]);
        s.inj.apply(&mut vars);
        assert_eq!(vars["CODEX_HOME"], broken.to_str().unwrap());
        assert!(!vars.contains_key("LOTSE_CODEX_SOURCE_HOME"));
    }

    #[test]
    fn lotse_codex_hooks_off_in_the_environment_keeps_codex_on_the_users_home() {
        let s = codex_setup();
        for off in ["0", "off", "false", "no"] {
            let mut vars = map(&[
                ("HOME", s.home.to_str().unwrap()),
                ("LOTSE_CODEX_HOOKS", off),
            ]);
            s.inj.apply(&mut vars);
            assert!(!vars.contains_key("CODEX_HOME"), "{off}");
        }
        let mut vars = map(&[
            ("HOME", s.home.to_str().unwrap()),
            ("LOTSE_CODEX_HOOKS", "1"),
        ]);
        s.inj.apply(&mut vars);
        assert!(vars.contains_key("CODEX_HOME"));
    }

    #[test]
    fn claude_gets_the_lotse_mcp_server_in_the_injected_plugin_dir() {
        let tmp = tempfile::tempdir().unwrap();
        let inj = Injection::prepare(tmp.path());
        let plugin = inj.claude_plugin_dir().unwrap();
        assert_eq!(
            std::fs::read_to_string(plugin.join(".mcp.json")).unwrap(),
            lotse_hook::mcp_inject::claude_mcp_json()
        );
    }

    #[test]
    fn codex_mirror_gets_the_lotse_mcp_server_pointing_at_the_hook_binary() {
        let s = codex_setup();
        let user_config = "model = \"gpt-5\"\n";
        std::fs::write(s.home.join(".codex/config.toml"), user_config).unwrap();
        let mut vars = map(&[
            ("HOME", s.home.to_str().unwrap()),
            (
                "LOTSE_HOOK_BIN",
                "/Applications/Lotse.app/Contents/MacOS/lotse-hook",
            ),
        ]);
        s.inj.apply(&mut vars);
        let mirror = PathBuf::from(&vars["CODEX_HOME"]);
        let config: toml::Table = std::fs::read_to_string(mirror.join("config.toml"))
            .unwrap()
            .parse()
            .unwrap();
        assert_eq!(config["model"].as_str(), Some("gpt-5"));
        let lotse = &config["mcp_servers"]["lotse"];
        assert_eq!(
            lotse["command"].as_str(),
            Some("/Applications/Lotse.app/Contents/MacOS/lotse-hook")
        );
        assert_eq!(lotse["args"][0].as_str(), Some("mcp"));
        // the hook trust entries are still there, and the user's own file is not touched
        assert!(config.contains_key("hooks"));
        assert_eq!(
            std::fs::read_to_string(s.home.join(".codex/config.toml")).unwrap(),
            user_config
        );
        // every pane applies it again: nothing changes
        let before = std::fs::read_to_string(mirror.join("config.toml")).unwrap();
        s.inj.apply(&mut vars);
        assert_eq!(
            std::fs::read_to_string(mirror.join("config.toml")).unwrap(),
            before
        );
    }

    #[test]
    fn opencode_gets_the_lotse_mcp_server_inline_and_a_users_inline_config_is_merged() {
        let tmp = tempfile::tempdir().unwrap();
        let inj = Injection::prepare(tmp.path());
        let mut vars = map(&[("LOTSE_HOOK_BIN", "/h/lotse-hook")]);
        inj.apply(&mut vars);
        let inline: serde_json::Value =
            serde_json::from_str(&vars["OPENCODE_CONFIG_CONTENT"]).unwrap();
        assert_eq!(
            inline["mcp"]["lotse"]["command"],
            serde_json::json!(["/h/lotse-hook", "mcp"])
        );
        // a pane started from a Lotse pane inherits it: unchanged
        let once = vars.clone();
        inj.apply(&mut vars);
        assert_eq!(vars, once);

        let mut vars = map(&[("OPENCODE_CONFIG_CONTENT", r#"{"theme":"x"}"#)]);
        inj.apply(&mut vars);
        let inline: serde_json::Value =
            serde_json::from_str(&vars["OPENCODE_CONFIG_CONTENT"]).unwrap();
        assert_eq!(inline["theme"], "x");
        assert_eq!(
            inline["mcp"]["lotse"]["command"][0], "lotse-hook",
            "no LOTSE_HOOK_BIN: found on PATH"
        );
        // a value that is not JSON is the user's: left alone
        let mut vars = map(&[("OPENCODE_CONFIG_CONTENT", "{// jsonc")]);
        inj.apply(&mut vars);
        assert_eq!(vars["OPENCODE_CONFIG_CONTENT"], "{// jsonc");
    }

    #[test]
    fn an_unwritable_data_dir_means_no_injection_not_a_failure() {
        let inj = Injection::prepare(Path::new("/proc/definitely/not/writable"));
        let mut vars = map(&[("CLAUDE_CODE_PLUGIN_DIRS", "/user/a")]);
        inj.apply(&mut vars);
        assert_eq!(vars, map(&[("CLAUDE_CODE_PLUGIN_DIRS", "/user/a")]));
    }
}
