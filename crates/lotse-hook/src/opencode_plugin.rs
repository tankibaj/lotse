//! Install / remove the OpenCode status plugin (`integrations/opencode/lotse-status.js`).
//!
//! The plugin is one file we own, copied to `<config dir>/plugins/lotse-status.js` where
//! `<config dir>` is `$XDG_CONFIG_HOME/opencode`, else `~/.config/opencode`. Same safety rules as
//! the Claude installer: idempotent, a foreign file with that name is never overwritten, a changed
//! file of ours is simply replaced (never backed up: a backup of our own file would come back as
//! stale code on uninstall), uninstall removes only a file that carries our marker and puts back
//! a `lotse-status.js.lotse-backup` only when that is a genuine non-Lotse file.
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
    if existing.as_deref().is_some_and(|old| !old.contains(MARKER)) {
        bail!(
            "{} exists and is not the Lotse plugin; not overwriting it",
            target.display()
        );
    }
    std::fs::create_dir_all(config_dir.join("plugins"))
        .with_context(|| format!("creating {}", config_dir.join("plugins").display()))?;
    // plugins are code the user's agent runs: readable by the user's tools, writable by the user only
    write_private(&target, PLUGIN_SOURCE)?;
    Ok(InstallOutcome::Written { backup: None })
}

pub fn uninstall(config_dir: &Path) -> Result<UninstallOutcome> {
    let target = plugin_path(config_dir);
    match read_existing(&target)? {
        Some(text) if text.contains(MARKER) => {}
        _ => return Ok(UninstallOutcome::NotInstalled),
    }
    std::fs::remove_file(&target).with_context(|| format!("removing {}", target.display()))?;
    let backup = backup_path(config_dir);
    match read_existing(&backup)? {
        Some(old) if !old.contains(MARKER) => {
            write_private(&target, &old)?;
            std::fs::remove_file(&backup).ok();
            return Ok(UninstallOutcome::RestoredBackup);
        }
        // A stale backup of our own file (older installers wrote one): never put it back.
        Some(_) => {
            std::fs::remove_file(&backup).ok();
        }
        None => {}
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

/// A per-pane `OPENCODE_CONFIG_DIR` that carries the plugin, so nothing is written to the user's
/// own config. OpenCode adds `$OPENCODE_CONFIG_DIR` to its config directories (its global one stays)
/// and loads `plugins/*.js` from each; verified in the installed OpenCode 1.19.4 binary, and Orca
/// does the same with an overlay (`src/main/opencode/hook-service.ts`).
///
/// `source` is a config directory the user already points `OPENCODE_CONFIG_DIR` at: setting the
/// variable replaces it, so the overlay symlinks each of its entries (and each entry of its
/// `plugins/`), and follows it on every call. Real files in the overlay (OpenCode's own `node_modules`)
/// are never touched. Returns the overlay's path, `<root>/default` or `<root>/<hash of source>`.
pub fn overlay(root: &Path, source: Option<&Path>) -> Result<PathBuf> {
    let name = source.map_or_else(
        || "default".to_owned(),
        |s| format!("{:016x}", fnv1a(s.as_os_str().as_encoded_bytes())),
    );
    let dir = root.join(name);
    let plugins = dir.join("plugins");
    std::fs::create_dir_all(&plugins).with_context(|| format!("creating {}", plugins.display()))?;
    let target = plugins.join(FILE_NAME);
    if read_existing(&target)?.as_deref() != Some(PLUGIN_SOURCE) {
        write_private(&target, PLUGIN_SOURCE)?;
    }
    if let Some(source) = source {
        mirror(source, &dir, None)?;
        mirror(&source.join("plugins"), &plugins, Some(FILE_NAME))?;
    }
    Ok(dir)
}

/// Make `into` hold a symlink for every entry of `from` (except `skip` and, at the top level, the
/// `plugins` directory, which is mirrored separately); drop symlinks whose entry is gone.
fn mirror(from: &Path, into: &Path, skip: Option<&str>) -> Result<()> {
    let wanted: Vec<std::ffi::OsString> = match std::fs::read_dir(from) {
        Ok(entries) => entries
            .filter_map(|e| e.ok().map(|e| e.file_name()))
            .filter(|n| Some(n.to_string_lossy().as_ref()) != skip)
            .filter(|n| skip.is_some() || n != "plugins")
            .collect(),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Vec::new(),
        Err(e) => return Err(e).with_context(|| format!("reading {}", from.display())),
    };
    for entry in std::fs::read_dir(into)?.filter_map(Result::ok) {
        let link = entry.path();
        if entry.file_type().is_ok_and(|t| t.is_symlink()) && !wanted.contains(&entry.file_name()) {
            std::fs::remove_file(&link).ok();
        }
    }
    for name in wanted {
        let link = into.join(&name);
        let to = from.join(&name);
        match std::fs::read_link(&link) {
            Ok(old) if old == to => continue,
            Ok(_) => std::fs::remove_file(&link)?,
            // a real file or directory of OpenCode's own: leave it
            Err(_) if link.symlink_metadata().is_ok() => continue,
            Err(_) => {}
        }
        std::os::unix::fs::symlink(&to, &link)
            .with_context(|| format!("linking {}", link.display()))?;
    }
    Ok(())
}

/// Stable across builds (unlike `DefaultHasher`): names the overlay directory of a source.
fn fnv1a(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0xcbf2_9ce4_8422_2325, |h, &b| {
        (h ^ u64::from(b)).wrapping_mul(0x0100_0000_01b3)
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
    fn an_older_copy_of_ours_is_overwritten_without_backup_and_removed_on_uninstall() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path();
        std::fs::create_dir_all(dir.join("plugins")).unwrap();
        let old = format!("// old version\n// reads {MARKER}\n");
        std::fs::write(plugin_path(dir), &old).unwrap();
        assert!(matches!(
            status(dir).unwrap(),
            InstallStatus::Partial { .. }
        ));

        assert_eq!(
            install(dir).unwrap(),
            InstallOutcome::Written { backup: None }
        );
        assert!(!backup_path(dir).exists());
        assert_eq!(status(dir).unwrap(), InstallStatus::Installed);
        assert_eq!(uninstall(dir).unwrap(), UninstallOutcome::Removed);
        assert!(!plugin_path(dir).exists());
    }

    #[test]
    fn uninstall_restores_a_genuine_user_backup_but_drops_a_stale_lotse_one() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path();
        std::fs::create_dir_all(dir.join("plugins")).unwrap();

        install(dir).unwrap();
        std::fs::write(backup_path(dir), "export default {}").unwrap();
        assert_eq!(uninstall(dir).unwrap(), UninstallOutcome::RestoredBackup);
        assert_eq!(
            std::fs::read_to_string(plugin_path(dir)).unwrap(),
            "export default {}"
        );

        std::fs::remove_file(plugin_path(dir)).unwrap();
        install(dir).unwrap();
        std::fs::write(backup_path(dir), format!("// stale {MARKER}")).unwrap();
        assert_eq!(uninstall(dir).unwrap(), UninstallOutcome::Removed);
        assert!(!plugin_path(dir).exists());
        assert!(!backup_path(dir).exists());
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

    #[test]
    fn overlay_without_a_user_dir_holds_only_the_plugin() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = overlay(tmp.path(), None).unwrap();
        assert_eq!(dir, overlay(tmp.path(), None).unwrap(), "stable path");
        assert_eq!(
            std::fs::read_to_string(plugin_path(&dir)).unwrap(),
            PLUGIN_SOURCE
        );
        assert_eq!(std::fs::read_dir(&dir).unwrap().count(), 1);
    }

    #[test]
    fn overlay_mirrors_the_users_config_dir_without_touching_it() {
        let tmp = tempfile::tempdir().unwrap();
        let user = tmp.path().join("user");
        std::fs::create_dir_all(user.join("plugins")).unwrap();
        std::fs::write(user.join("opencode.json"), "{}").unwrap();
        std::fs::write(user.join("plugins/mine.js"), "export default {}").unwrap();
        // a foreign file with our name must not shadow ours in the overlay
        std::fs::write(user.join("plugins").join(FILE_NAME), "// theirs").unwrap();
        let root = tmp.path().join("overlays");

        let dir = overlay(&root, Some(&user)).unwrap();
        assert_ne!(dir, overlay(&root, None).unwrap(), "one overlay per source");
        assert_eq!(
            std::fs::read_to_string(dir.join("opencode.json")).unwrap(),
            "{}"
        );
        assert_eq!(
            std::fs::read_to_string(dir.join("plugins/mine.js")).unwrap(),
            "export default {}"
        );
        assert_eq!(
            std::fs::read_to_string(plugin_path(&dir)).unwrap(),
            PLUGIN_SOURCE
        );
        assert_eq!(
            std::fs::read_to_string(user.join("plugins").join(FILE_NAME)).unwrap(),
            "// theirs",
            "the user's directory is never written"
        );

        // the user removes a file and adds another: the next overlay call follows
        std::fs::remove_file(user.join("opencode.json")).unwrap();
        std::fs::write(user.join("AGENTS.md"), "x").unwrap();
        assert_eq!(overlay(&root, Some(&user)).unwrap(), dir);
        assert!(!dir.join("opencode.json").exists());
        assert!(dir.join("AGENTS.md").exists());
    }

    #[test]
    fn a_missing_user_dir_is_just_the_plugin() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = overlay(tmp.path(), Some(&tmp.path().join("nope"))).unwrap();
        assert!(plugin_path(&dir).is_file());
    }
}
