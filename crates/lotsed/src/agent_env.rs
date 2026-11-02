//! What a pane's environment says about coding agents.
//!
//! * [`scrub_host_markers`]: Lotse may itself have been started from inside an agent (Claude Code's
//!   Bash tool, a Codex session, …). The markers that agent exports to its children would make the
//!   agent in a Lotse pane believe it is a child session, so they are dropped. User configuration
//!   (`ANTHROPIC_*`, `CODEX_HOME`, `CLAUDE_CONFIG_DIR`, `CLAUDE_CODE_USE_BEDROCK`, …) stays.
//! * [`Injection`]: status hooks that need no global install. Claude Code gets the Lotse plugin
//!   through `CLAUDE_CODE_PLUGIN_DIRS`, OpenCode gets the Lotse plugin through an overlay
//!   `OPENCODE_CONFIG_DIR`; both are per pane, nothing in `~/.claude` or `~/.config/opencode` changes.
//!   Codex is not injected: its hooks need a trusted entry in `config.toml`, so they would have to
//!   live in the user's `~/.codex` or in a mirrored `CODEX_HOME` (see `integrations/codex/README.md`).

use lotse_hook::{claude_plugin, opencode_plugin};
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

/// Plugins written at daemon start, and how they reach a pane.
#[derive(Debug, Clone, Default)]
pub struct Injection {
    claude_plugin: Option<PathBuf>,
    opencode_overlays: Option<PathBuf>,
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
            Ok(dir) => Some(dir),
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
        Self {
            claude_plugin,
            opencode_overlays,
        }
    }

    /// Put the injection into a pane's environment.
    pub fn apply(&self, vars: &mut HashMap<String, String>) {
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
        }
    }
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

    #[test]
    fn an_unwritable_data_dir_means_no_injection_not_a_failure() {
        let inj = Injection::prepare(Path::new("/proc/definitely/not/writable"));
        let mut vars = map(&[("CLAUDE_CODE_PLUGIN_DIRS", "/user/a")]);
        inj.apply(&mut vars);
        assert_eq!(vars, map(&[("CLAUDE_CODE_PLUGIN_DIRS", "/user/a")]));
    }
}
