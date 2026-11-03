//! Tauri shell: the bridge between the webview and `lotsed` (see [`daemon`]), host commands
//! ([`host`]), installed fonts ([`fonts`]), SF Symbols ([`symbols`]), the app menu ([`menu`]), notifications and the Dock badge
//! ([`notify`]). Beta stubs, filled in by later work packages: [`un`] (actionable notifications), [`export`],
//! [`config_open`], [`speech`].

pub mod config_open;
pub mod daemon;
pub mod export;
pub mod fonts;
pub mod host;
pub mod menu;
pub mod notify;
pub mod speech;
pub mod symbols;
pub mod un;

use std::sync::Arc;
use tauri::Manager;

/// `--isolated-data-dir DIR`: talk to (and start) a daemon that keeps its data in `DIR` and does not
/// source the login shell. For development and smoke runs that must not touch the real data.
fn bridge_config(args: impl Iterator<Item = String>) -> daemon::Config {
    let mut config = daemon::Config::default();
    let mut args = args;
    while let Some(arg) = args.next() {
        if arg == "--isolated-data-dir"
            && let Some(dir) = args.next()
        {
            config.data_dir = Some(dir.into());
            config.daemon_args.push("--no-login-env".into());
        }
    }
    config
}

pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_notification::init())
        .manage(notify::NotifyState::default())
        .manage(Arc::new(daemon::Bridge::with_config(bridge_config(
            std::env::args().skip(1),
        ))))
        .setup(|app| {
            // the sink needs the app handle, which exists only now
            let sink = Arc::new(daemon::TauriSink::new(app.handle().clone()));
            app.state::<Arc<daemon::Bridge>>().set_sink(sink.clone());
            app.manage(sink);
            app.set_menu(menu::build(app.handle())?)?;
            un::init(app.handle())?;
            Ok(())
        })
        .on_menu_event(menu::on_menu_event)
        .invoke_handler(tauri::generate_handler![
            daemon::daemon_connect,
            daemon::daemon_request,
            daemon::daemon_pty_write,
            notify::set_badge,
            notify::set_focused_session,
            host::scan_repos,
            host::clone_repo,
            host::create_project_dir,
            host::open_url,
            host::reveal_in_finder,
            host::integration_status,
            host::integration_uninstall,
            fonts::list_fonts,
            symbols::symbol_png,
            un::un_available,
            export::export_write,
            config_open::open_agent_config,
            speech::speech_available,
            speech::speech_start,
            speech::speech_stop,
        ])
        .run(tauri::generate_context!())
        .expect("error while running Lotse");
}
