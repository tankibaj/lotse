//! Transcript export: writes the text the user chose to save to the path they picked in the save dialog.
//! The webview only learns the path from the save dialog; the command still takes nothing but an absolute
//! path to a file in an existing folder, and writes a temp file next to it before the rename.

use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::Path;

/// Write `content` to the file the user picked: `path` must be absolute, not a directory, in an existing folder.
#[tauri::command]
pub fn export_write(path: String, content: String) -> Result<(), String> {
    if path.trim().is_empty() {
        return Err("export: no path given".into());
    }
    write_file(Path::new(&path), &content)
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
    let (Some(dir), Some(name)) = (path.parent(), path.file_name()) else {
        return Err(format!("export: {} is not a file path", path.display()));
    };
    if !dir.is_dir() {
        return Err(format!("export: {} does not exist", dir.display()));
    }
    let tmp = dir.join(format!(
        ".{}.lotse-{}.tmp",
        name.to_string_lossy(),
        std::process::id()
    ));
    let written = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&tmp)
        .and_then(|mut f| f.write_all(content.as_bytes()).and_then(|()| f.sync_all()))
        .and_then(|()| fs::rename(&tmp, path));
    written.map_err(|e| {
        let _ = fs::remove_file(&tmp);
        format!("export: {}: {e}", path.display())
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn tmp() -> tempfile::TempDir {
        tempfile::tempdir().unwrap()
    }

    fn path(dir: &tempfile::TempDir, name: &str) -> String {
        dir.path().join(name).to_string_lossy().into_owned()
    }

    #[test]
    fn writes_the_content_to_the_chosen_path() {
        let d = tmp();
        let p = path(&d, "fix.md");
        export_write(p.clone(), "# Fix\n".into()).unwrap();
        assert_eq!(fs::read_to_string(&p).unwrap(), "# Fix\n");
    }

    #[test]
    fn replaces_an_existing_file_and_leaves_no_temp_file_behind() {
        let d = tmp();
        let p = path(&d, "fix.md");
        fs::write(&p, "old").unwrap();
        export_write(p.clone(), "new".into()).unwrap();
        assert_eq!(fs::read_to_string(&p).unwrap(), "new");
        let names: Vec<_> = fs::read_dir(d.path())
            .unwrap()
            .map(|e| e.unwrap().file_name())
            .collect();
        assert_eq!(names, ["fix.md"]);
    }

    #[test]
    fn refuses_a_directory() {
        let d = tmp();
        let err = export_write(d.path().to_string_lossy().into_owned(), "x".into()).unwrap_err();
        assert!(err.contains("directory"), "{err}");
        assert_eq!(fs::read_dir(d.path()).unwrap().count(), 0);
    }

    #[test]
    fn refuses_an_empty_path() {
        assert!(export_write(String::new(), "x".into()).is_err());
        assert!(export_write("   ".into(), "x".into()).is_err());
    }

    #[test]
    fn refuses_a_relative_path_and_a_missing_folder() {
        assert!(export_write("fix.md".into(), "x".into()).is_err());
        let d = tmp();
        assert!(export_write(path(&d, "nope/fix.md"), "x".into()).is_err());
    }

    #[test]
    fn writes_an_empty_document() {
        let d = tmp();
        let p = path(&d, "empty.txt");
        export_write(p.clone(), String::new()).unwrap();
        assert_eq!(fs::read_to_string(&p).unwrap(), "");
    }
}
