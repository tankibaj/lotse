//! Transcript export: writes the text the user chose to save to the path they picked in the save dialog.
//! Stub: wp15 fills it in (user-chosen path only, refuses directories, temp file then rename).

/// Write `content` to `path`. Not implemented yet.
#[tauri::command]
pub fn export_write(_path: String, _content: String) -> Result<(), String> {
    Err("export is not implemented yet".into())
}
