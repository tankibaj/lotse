//! UNUserNotificationCenter (actionable notifications). Stub: wp09 fills it in. [`init`] runs once from the
//! Tauri `setup`; until then it does nothing and the plugin notifications of [`crate::notify`] stay in charge.

/// Register the notification categories and the response delegate. No-op until wp09.
pub fn init(_app: &tauri::AppHandle) -> tauri::Result<()> {
    Ok(())
}

/// Whether actionable notifications are wired up (false until wp09).
#[tauri::command]
pub fn un_available() -> bool {
    false
}
