//! Tauri shell. WP0 skeleton; WP8 wires the daemon client bridge:
//! Tauri commands for requests, Tauri events for state, `tauri::ipc::Channel` for PTY output.

pub fn run() {
    tauri::Builder::default()
        .run(tauri::generate_context!())
        .expect("error while running Lotse");
}
