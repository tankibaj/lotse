//! `lotse-hook`: agents' hooks call this; it forwards the hook JSON to the `lotsed` socket.
//!
//! ```text
//! lotse-hook [claude|codex|opencode]                 hook mode: stdin JSON -> lotsed (default claude)
//! lotse-hook integration claude|opencode install|uninstall|status [--config-dir DIR]
//! ```
//! Hook mode reads `LOTSE_SESSION_ID`, `LOTSE_SOCKET` and `LOTSE_TOKEN_FILE` (set by the daemon on
//! every pane); the token is read from that file, never from the environment.
//!
//! Hook mode prints nothing and always exits 0 (see `lotse_hook::forward`); set `LOTSE_HOOK_DEBUG`
//! to see why an event was not delivered on stderr.

use lotse_hook::claude_settings::{self, InstallOutcome, InstallStatus, UninstallOutcome};
use lotse_hook::forward::{self, Config};
use lotse_hook::opencode_plugin;
use lotse_proto::AgentKind;
use std::ffi::OsString;
use std::path::PathBuf;
use std::process::ExitCode;

const USAGE: &str = "usage: lotse-hook [claude|codex|opencode]\n       lotse-hook integration claude|opencode install|uninstall|status [--config-dir DIR]";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        Some("integration") => integration(&args[1..]),
        Some("-h" | "--help") => {
            println!("{USAGE}");
            ExitCode::SUCCESS
        }
        Some("-V" | "--version") => {
            println!(
                "lotse-hook {} (proto {})",
                env!("CARGO_PKG_VERSION"),
                lotse_proto::PROTO_VERSION
            );
            ExitCode::SUCCESS
        }
        other => {
            hook(other.unwrap_or("claude"));
            // Never fail the agent, whatever happened.
            ExitCode::SUCCESS
        }
    }
}

fn hook(agent_arg: &str) {
    let debug = std::env::var_os("LOTSE_HOOK_DEBUG").is_some();
    let agent = match agent_arg {
        "claude" => AgentKind::ClaudeCode,
        "codex" => AgentKind::Codex,
        "opencode" => AgentKind::OpenCode,
        _ => return,
    };
    let Some(cfg) = Config::from_env(agent, |name| std::env::var_os(name)) else {
        return;
    };
    if let Err(e) = forward::run(&cfg, std::io::stdin().lock())
        && debug
    {
        eprintln!("lotse-hook: {e:#}");
    }
}

fn integration(args: &[String]) -> ExitCode {
    match run_integration(args) {
        Ok(()) => ExitCode::SUCCESS,
        Err(message) => {
            eprintln!("lotse-hook: {message}\n{USAGE}");
            ExitCode::FAILURE
        }
    }
}

fn run_integration(args: &[String]) -> Result<(), String> {
    let [target, action, rest @ ..] = args else {
        return Err("missing arguments".into());
    };
    let opencode = match target.as_str() {
        "claude" => false,
        "opencode" => true,
        _ => return Err(format!("unknown integration {target:?}")),
    };
    let dir = match rest {
        [] if opencode => opencode_plugin::config_dir(
            std::env::var_os("XDG_CONFIG_HOME").as_deref(),
            dirs::home_dir().as_deref(),
        )
        .ok_or("cannot find the home directory; pass --config-dir")?,
        [] => claude_settings::config_dir(
            std::env::var_os("CLAUDE_CONFIG_DIR").as_deref(),
            dirs::home_dir().as_deref(),
        )
        .ok_or("cannot find the home directory; pass --config-dir")?,
        [flag, dir] if flag == "--config-dir" => PathBuf::from(OsString::from(dir)),
        _ => return Err("unexpected arguments".into()),
    };
    let report = |e: anyhow::Error| format!("{e:#}");
    if opencode {
        match action.as_str() {
            "install" => match opencode_plugin::install(&dir).map_err(report)? {
                InstallOutcome::Unchanged => println!("plugin already installed"),
                InstallOutcome::Written { backup: Some(b) } => {
                    println!("plugin installed; backup: {}", b.display());
                }
                InstallOutcome::Written { backup: None } => println!("plugin installed"),
            },
            "uninstall" => match opencode_plugin::uninstall(&dir).map_err(report)? {
                UninstallOutcome::NotInstalled => println!("no Lotse plugin found"),
                UninstallOutcome::Removed => println!("plugin removed"),
                UninstallOutcome::RestoredBackup => {
                    println!("plugin removed; previous file restored from backup");
                }
            },
            "status" => match opencode_plugin::status(&dir).map_err(report)? {
                InstallStatus::NotInstalled => println!("not installed"),
                InstallStatus::Installed => println!("installed"),
                InstallStatus::Partial { missing } => {
                    println!("outdated; missing: {}", missing.join(", "));
                }
            },
            other => return Err(format!("unknown action {other:?}")),
        }
        return Ok(());
    }
    match action.as_str() {
        "install" => {
            let hook_path = std::env::current_exe()
                .and_then(|p| p.canonicalize())
                .map_err(|e| format!("cannot locate the lotse-hook binary: {e}"))?;
            match claude_settings::install(&dir, &hook_path).map_err(report)? {
                InstallOutcome::Unchanged => println!("hooks already installed"),
                InstallOutcome::Written { backup: Some(b) } => {
                    println!("hooks installed; backup: {}", b.display());
                }
                InstallOutcome::Written { backup: None } => println!("hooks installed"),
            }
        }
        "uninstall" => match claude_settings::uninstall(&dir).map_err(report)? {
            UninstallOutcome::NotInstalled => println!("no Lotse hooks found"),
            UninstallOutcome::Removed => println!("hooks removed"),
            UninstallOutcome::RestoredBackup => {
                println!("hooks removed; settings restored from backup")
            }
        },
        "status" => match claude_settings::status(&dir).map_err(report)? {
            InstallStatus::NotInstalled => println!("not installed"),
            InstallStatus::Installed => println!("installed"),
            InstallStatus::Partial { missing } => {
                println!("partial; missing: {}", missing.join(", "))
            }
        },
        other => return Err(format!("unknown action {other:?}")),
    }
    Ok(())
}
