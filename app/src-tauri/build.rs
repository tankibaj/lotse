fn main() {
    // `tauri::generate_context!` embeds `frontendDist` at compile time. Keep `cargo clippy/test`
    // working before the frontend was built (CI runs cargo first).
    let _ = std::fs::create_dir_all("../dist");
    tauri_build::build();
}
