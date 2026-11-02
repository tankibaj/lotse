//! Install / remove the OpenCode status plugin (`integrations/opencode/lotse-status.js`).
//!
//! The plugin is one file we own, copied to `<config dir>/plugins/lotse-status.js` where
//! `<config dir>` is `$XDG_CONFIG_HOME/opencode`, else `~/.config/opencode`. Same safety rules as
//! the Claude installer: idempotent, a foreign file with that name is never overwritten, a changed
//! file of ours is backed up first (`lotse-status.js.lotse-backup`, mode 0600), uninstall removes only
//! a file that carries our marker (and puts the backup back when there is one).
//!
//! Unverified (no web access): that OpenCode loads plugins from this directory,
//! see <https://opencode.ai/docs/plugins/>.

use crate::claude_settings::{InstallOutcome, InstallStatus, UninstallOutcome};
use anyhow::{Context, Result, bail};
use std::ffi::OsStr;
use std::path::{Path, PathBuf};

/// The plugin source, embedded at build time.
pub const PLUGIN_SOURCE: &str = include_str!("../../../integrations/opencode/lotse-status.js");

/// Appears in the plugin source (it reads this variable); marks a file as ours.
const MARKER: &str = "LOTSE_SESSION_ID";
const FILE_NAME: &str = "lotse-status.js";

/// `$XDG_CONFIG_HOME/opencode`, else `<home>/.config/opencode`.
pub fn config_dir(xdg_config_home: Option<&OsStr>, home: Option<&Path>) -> Option<PathBuf> {
    match xdg_config_home.filter(|d| !d.is_empty()) {
        Some(dir) => Some(PathBuf::from(dir).join("opencode")),
        None => Some(home?.join(".config").join("opencode")),
    }
}

pub fn plugin_path(config_dir: &Path) -> PathBuf {
    config_dir.join("plugins").join(FILE_NAME)
}

pub fn backup_path(config_dir: &Path) -> PathBuf {
    config_dir
        .join("plugins")
        .join(format!("{FILE_NAME}.lotse-backup"))
}

fn read_existing(path: &Path) -> Result<Option<String>> {
    match std::fs::read_to_string(path) {
        Ok(text) => Ok(Some(text)),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e).with_context(|| format!("reading {}", path.display())),
    }
}

fn write_private(path: &Path, text: &str) -> Result<()> {
    use std::io::Write;
    use std::os::unix::fs::OpenOptionsExt;
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .mode(0o600)
        .open(path)
        .with_context(|| format!("writing {}", path.display()))?;
    file.write_all(text.as_bytes())?;
    Ok(())
}

pub fn install(config_dir: &Path) -> Result<InstallOutcome> {
    let target = plugin_path(config_dir);
    let existing = read_existing(&target)?;
    if existing.as_deref() == Some(PLUGIN_SOURCE) {
        return Ok(InstallOutcome::Unchanged);
    }
    let mut backup = None;
    if let Some(old) = &existing {
        if !old.contains(MARKER) {
            bail!(
                "{} exists and is not the Lotse plugin; not overwriting it",
                target.display()
            );
        }
        let path = backup_path(config_dir);
        write_private(&path, old)?;
        backup = Some(path);
    }
    std::fs::create_dir_all(config_dir.join("plugins"))
        .with_context(|| format!("creating {}", config_dir.join("plugins").display()))?;
    // plugins are code the user's agent runs: readable by the user's tools, writable by the user only
    write_private(&target, PLUGIN_SOURCE)?;
    Ok(InstallOutcome::Written { backup })
}

pub fn uninstall(config_dir: &Path) -> Result<UninstallOutcome> {
    let target = plugin_path(config_dir);
    match read_existing(&target)? {
        Some(text) if text.contains(MARKER) => {}
        _ => return Ok(UninstallOutcome::NotInstalled),
    }
    std::fs::remove_file(&target).with_context(|| format!("removing {}", target.display()))?;
    let backup = backup_path(config_dir);
    if let Some(old) = read_existing(&backup)? {
        write_private(&target, &old)?;
        std::fs::remove_file(&backup).ok();
        return Ok(UninstallOutcome::RestoredBackup);
    }
    Ok(UninstallOutcome::Removed)
}

pub fn status(config_dir: &Path) -> Result<InstallStatus> {
    Ok(match read_existing(&plugin_path(config_dir))? {
        Some(text) if text == PLUGIN_SOURCE => InstallStatus::Installed,
        Some(text) if text.contains(MARKER) => InstallStatus::Partial {
            missing: vec!["current plugin version".into()],
        },
        _ => InstallStatus::NotInstalled,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn config_dir_prefers_xdg_then_home() {
        let home = Path::new("/h");
        assert_eq!(
            config_dir(Some(OsStr::new("/x")), Some(home)),
            Some(PathBuf::from("/x/opencode"))
        );
        assert_eq!(
            config_dir(Some(OsStr::new("")), Some(home)),
            Some(PathBuf::from("/h/.config/opencode"))
        );
        assert_eq!(config_dir(None, None), None);
    }

    #[test]
    fn install_is_idempotent_and_uninstall_removes_only_ours() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path();
        assert_eq!(status(dir).unwrap(), InstallStatus::NotInstalled);
        assert_eq!(uninstall(dir).unwrap(), UninstallOutcome::NotInstalled);

        assert_eq!(
            install(dir).unwrap(),
            InstallOutcome::Written { backup: None }
        );
        assert_eq!(status(dir).unwrap(), InstallStatus::Installed);
        assert_eq!(install(dir).unwrap(), InstallOutcome::Unchanged);
        assert_eq!(
            std::fs::read_to_string(plugin_path(dir)).unwrap(),
            PLUGIN_SOURCE
        );

        assert_eq!(uninstall(dir).unwrap(), UninstallOutcome::Removed);
        assert!(!plugin_path(dir).exists());
    }

    #[test]
    fn an_older_copy_of_ours_is_backed_up_and_restored_on_uninstall() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path();
        std::fs::create_dir_all(dir.join("plugins")).unwrap();
        let old = format!("// old version\n// reads {MARKER}\n");
        std::fs::write(plugin_path(dir), &old).unwrap();
        assert!(matches!(
            status(dir).unwrap(),
            InstallStatus::Partial { .. }
        ));

        let InstallOutcome::Written { backup: Some(b) } = install(dir).unwrap() else {
            panic!("expected a backup");
        };
        assert_eq!(std::fs::read_to_string(b).unwrap(), old);
        assert_eq!(uninstall(dir).unwrap(), UninstallOutcome::RestoredBackup);
        assert_eq!(std::fs::read_to_string(plugin_path(dir)).unwrap(), old);
    }

    #[test]
    fn a_foreign_file_is_never_touched() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path();
        std::fs::create_dir_all(dir.join("plugins")).unwrap();
        std::fs::write(plugin_path(dir), "export default {}").unwrap();
        assert!(install(dir).is_err());
        assert_eq!(uninstall(dir).unwrap(), UninstallOutcome::NotInstalled);
        assert_eq!(
            std::fs::read_to_string(plugin_path(dir)).unwrap(),
            "export default {}"
        );
    }
}
