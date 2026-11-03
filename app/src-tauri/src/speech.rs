//! Voice input for the composer (Apple Speech). Stub: wp19 fills it in, and only if the Dictation spike
//! (`docs/spike/beta-voice-dictation-*.md`) says the system Dictation is not enough.

/// Whether on-device speech input is available (false until wp19).
#[tauri::command]
pub fn speech_available() -> bool {
    false
}

/// Start listening. Not implemented yet.
#[tauri::command]
pub fn speech_start() -> Result<(), String> {
    Err("speech input is not implemented yet".into())
}

/// Stop listening. Not implemented yet.
#[tauri::command]
pub fn speech_stop() -> Result<(), String> {
    Err("speech input is not implemented yet".into())
}
