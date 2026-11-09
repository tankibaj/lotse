//! Transcript export: writes the text the user chose to save to the path they picked in the save dialog.
//! The save dialog runs here, in Rust, and the file goes only to the path it returns: the webview supplies a
//! suggested name and the content, never a path, so a compromised page cannot overwrite arbitrary files.
//! The content is written to a private temp file next to the target, then renamed over it.

use std::collections::hash_map::RandomState;
use std::fs::{self, OpenOptions};
use std::hash::{BuildHasher, Hasher};
use std::io::{ErrorKind, Write};
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use tauri::Manager;
use tauri_plugin_dialog::DialogExt;

const TEMP_ATTEMPTS: usize = 8;

/// Ask the user where to save, then write `content` there. The saved path, or `None` when they cancelled.
/// The dialog is a sheet on the main window; a second request while one is open is refused.
#[tauri::command]
pub async fn export_save(
    app: tauri::AppHandle,
    file_name: String,
    content: String,
) -> Result<Option<String>, String> {
    static OPEN: AtomicBool = AtomicBool::new(false);
    let _guard = DialogGuard::acquire(&OPEN)?;
    let pick = |name: &str| {
        let mut dialog = app
            .dialog()
            .file()
            .set_title("Export transcript")
            .set_file_name(name);
        if let Some(main) = app.get_webview_window("main") {
            dialog = dialog.set_parent(&main);
        }
        dialog
            .blocking_save_file()
            .map(|p| {
                p.into_path()
                    .map_err(|e| format!("export: the dialog returned no file path: {e}"))
            })
            .transpose()
    };
    let saved = export_with(pick, &file_name, &content)?;
    Ok(saved.map(|p| p.to_string_lossy().into_owned()))
}

/// Held while a save dialog is open.
struct DialogGuard<'a>(&'a AtomicBool);

impl<'a> DialogGuard<'a> {
    fn acquire(open: &'a AtomicBool) -> Result<Self, String> {
        if open.swap(true, Ordering::AcqRel) {
            return Err("export: a save dialog is already open".into());
        }
        Ok(Self(open))
    }
}

impl Drop for DialogGuard<'_> {
    fn drop(&mut self) {
        self.0.store(false, Ordering::Release);
    }
}

/// `pick` shows the dialog: `Ok(None)` when the user cancelled, `Err` when the result is not a file path.
fn export_with(
    pick: impl FnOnce(&str) -> Result<Option<PathBuf>, String>,
    file_name: &str,
    content: &str,
) -> Result<Option<PathBuf>, String> {
    let Some(path) = pick(&default_name(file_name))? else {
        return Ok(None);
    };
    write_file(&path, content)?;
    Ok(Some(path))
}

/// The suggested name: its last path component only.
fn default_name(file_name: &str) -> String {
    Path::new(file_name)
        .file_name()
        .map_or_else(|| "transcript".into(), |n| n.to_string_lossy().into_owned())
}

fn write_file(path: &Path, content: &str) -> Result<(), String> {
    if !path.is_absolute() {
        return Err(format!(
            "export: {} is not an absolute path",
            path.display()
        ));
    }
    if path.is_dir() {
        return Err(format!("export: {} is a directory", path.display()));
    }
    let (Some(dir), Some(_)) = (path.parent(), path.file_name()) else {
        return Err(format!("export: {} is not a file path", path.display()));
    };
    if !dir.is_dir() {
        return Err(format!("export: {} does not exist", dir.display()));
    }
    write_atomic(path, content, temp_name)
}

/// A short unpredictable name, independent of the target's (which may already be at the name-length limit).
fn temp_name() -> String {
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let mut h = RandomState::new().build_hasher();
    h.write_u64(COUNTER.fetch_add(1, Ordering::Relaxed));
    h.write_u32(std::process::id());
    format!(".lotse-export-{:016x}.tmp", h.finish())
}

/// Write to a new temp file in `path`'s folder, then rename it over `path`. The temp file takes the existing
/// target's permissions; a new file is 0600 (a transcript can hold what redaction missed). A symlink at `path`
/// is replaced, never written through. A temp name that is taken is skipped; only a file this call created is
/// ever removed.
fn write_atomic(
    path: &Path,
    content: &str,
    mut next_name: impl FnMut() -> String,
) -> Result<(), String> {
    let fail = |e: std::io::Error| format!("export: {}: {e}", path.display());
    let dir = path.parent().unwrap_or(Path::new("/"));
    let existing = fs::metadata(path).ok().map(|m| m.permissions());
    for _ in 0..TEMP_ATTEMPTS {
        let tmp = dir.join(next_name());
        let mut file = match OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&tmp)
        {
            Ok(f) => f,
            Err(e) if e.kind() == ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(fail(e)),
        };
        let written = file
            .write_all(content.as_bytes())
            .and_then(|()| existing.clone().map_or(Ok(()), |p| file.set_permissions(p)))
            .and_then(|()| file.sync_all())
            .and_then(|()| fs::rename(&tmp, path));
        if let Err(e) = written {
            let _ = fs::remove_file(&tmp);
            return Err(fail(e));
        }
        return Ok(());
    }
    Err(fail(std::io::Error::new(
        ErrorKind::AlreadyExists,
        "no free temp file name",
    )))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::os::unix::fs::PermissionsExt;
    use std::path::PathBuf;

    fn tmp() -> tempfile::TempDir {
        tempfile::tempdir().unwrap()
    }

    fn picks(p: PathBuf) -> impl FnOnce(&str) -> Result<Option<PathBuf>, String> {
        move |_| Ok(Some(p))
    }

    #[test]
    fn an_existing_files_permissions_survive_the_rewrite() {
        let d = tmp();
        let p = d.path().join("fix.md");
        fs::write(&p, "old").unwrap();
        fs::set_permissions(&p, fs::Permissions::from_mode(0o640)).unwrap();
        export_with(picks(p.clone()), "fix.md", "new").unwrap();
        assert_eq!(fs::read_to_string(&p).unwrap(), "new");
        assert_eq!(
            fs::metadata(&p).unwrap().permissions().mode() & 0o777,
            0o640
        );
    }

    #[test]
    fn a_symlink_is_replaced_and_its_target_is_not_written() {
        let d = tmp();
        let real = d.path().join("real.txt");
        fs::write(&real, "precious").unwrap();
        let link = d.path().join("link.md");
        std::os::unix::fs::symlink(&real, &link).unwrap();
        export_with(picks(link.clone()), "link.md", "new").unwrap();
        assert_eq!(fs::read_to_string(&real).unwrap(), "precious");
        assert_eq!(fs::read_to_string(&link).unwrap(), "new");
        assert!(
            !fs::symlink_metadata(&link)
                .unwrap()
                .file_type()
                .is_symlink()
        );
    }

    #[test]
    fn a_dialog_result_that_is_not_a_path_is_an_error_not_a_cancel() {
        let err = export_with(|_| Err("not a file path".into()), "x", "x").unwrap_err();
        assert!(err.contains("not a file path"), "{err}");
    }

    #[test]
    fn only_one_save_dialog_is_open_at_a_time() {
        let open = AtomicBool::new(false);
        let first = DialogGuard::acquire(&open).unwrap();
        assert!(DialogGuard::acquire(&open).is_err());
        drop(first);
        assert!(DialogGuard::acquire(&open).is_ok());
    }

    #[test]
    fn writes_the_content_to_the_path_the_dialog_returned() {
        let d = tmp();
        let p = d.path().join("fix.md");
        let saved = export_with(picks(p.clone()), "fix.md", "# Fix\n").unwrap();
        assert_eq!(saved, Some(p.clone()));
        assert_eq!(fs::read_to_string(&p).unwrap(), "# Fix\n");
    }

    #[test]
    fn a_cancelled_dialog_writes_nothing() {
        assert_eq!(export_with(|_| Ok(None), "fix.md", "x").unwrap(), None);
    }

    #[test]
    fn the_dialog_only_gets_a_bare_file_name_as_its_default() {
        let seen = std::cell::RefCell::new(String::new());
        export_with(
            |n| {
                *seen.borrow_mut() = n.to_string();
                Ok(None)
            },
            "../../.zshrc",
            "x",
        )
        .unwrap();
        assert_eq!(*seen.borrow(), ".zshrc");
        assert_eq!(default_name("/etc/"), "etc");
        assert_eq!(default_name(".."), "transcript");
        assert_eq!(default_name(""), "transcript");
    }

    #[test]
    fn replaces_an_existing_file_and_leaves_no_temp_file_behind() {
        let d = tmp();
        let p = d.path().join("fix.md");
        fs::write(&p, "old").unwrap();
        export_with(picks(p.clone()), "fix.md", "new").unwrap();
        assert_eq!(fs::read_to_string(&p).unwrap(), "new");
        let names: Vec<_> = fs::read_dir(d.path())
            .unwrap()
            .map(|e| e.unwrap().file_name())
            .collect();
        assert_eq!(names, ["fix.md"]);
    }

    #[test]
    fn refuses_a_directory_a_relative_path_and_a_missing_folder() {
        let d = tmp();
        let err = export_with(picks(d.path().to_path_buf()), "x", "x").unwrap_err();
        assert!(err.contains("directory"), "{err}");
        assert_eq!(fs::read_dir(d.path()).unwrap().count(), 0);
        assert!(export_with(picks("fix.md".into()), "x", "x").is_err());
        assert!(export_with(picks(d.path().join("nope/fix.md")), "x", "x").is_err());
        assert!(export_with(picks(PathBuf::new()), "x", "x").is_err());
    }

    #[test]
    fn writes_an_empty_document() {
        let d = tmp();
        let p = d.path().join("empty.txt");
        export_with(picks(p.clone()), "empty.txt", "").unwrap();
        assert_eq!(fs::read_to_string(&p).unwrap(), "");
    }

    #[test]
    fn the_file_is_private_to_the_user() {
        let d = tmp();
        let p = d.path().join("fix.md");
        export_with(picks(p.clone()), "fix.md", "x").unwrap();
        assert_eq!(
            fs::metadata(&p).unwrap().permissions().mode() & 0o777,
            0o600
        );
    }

    #[test]
    fn a_name_at_the_filesystem_limit_still_saves() {
        let d = tmp();
        let p = d.path().join("a".repeat(255));
        export_with(picks(p.clone()), "a", "x").unwrap();
        assert_eq!(fs::read_to_string(&p).unwrap(), "x");
        assert_eq!(fs::read_dir(d.path()).unwrap().count(), 1);
    }

    #[test]
    fn a_taken_temp_name_is_skipped_and_the_other_file_is_left_alone() {
        let d = tmp();
        let other = d.path().join(".lotse-export-0.tmp");
        fs::write(&other, "someone else's").unwrap();
        let p = d.path().join("fix.md");
        let mut names = [".lotse-export-0.tmp", ".lotse-export-1.tmp"].into_iter();
        write_atomic(&p, "mine", || names.next().unwrap().to_string()).unwrap();
        assert_eq!(fs::read_to_string(&p).unwrap(), "mine");
        assert_eq!(fs::read_to_string(&other).unwrap(), "someone else's");
    }

    #[test]
    fn temp_names_differ_and_stay_short() {
        let (a, b) = (temp_name(), temp_name());
        assert_ne!(a, b);
        assert!(a.len() < 64, "{a}");
    }
}
