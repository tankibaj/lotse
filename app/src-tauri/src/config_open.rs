//! "Manage" in the session panel's connector list: opens an agent config file in the default text editor.
//! Stub: wp16 fills it in (`open -t`, only for a path the daemon listed).

/// Open the agent config at `path`. Not implemented yet.
#[tauri::command]
pub fn open_agent_config(_path: String) -> Result<(), String> {
    Err("opening agent configs is not implemented yet".into())
}
