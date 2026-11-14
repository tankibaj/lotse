//! The application menu, built explicitly (Tauri's default menu has no "Settings…"). The app menu
//! carries the standard macOS items plus **Settings… ⌘,**; File, Edit, View and Window keep the
//! predefined items of the default menu (Edit's Undo/Redo/Cut/Copy/Paste/Select All must keep
//! working in inputs and xterm).
//!
//! "Settings…" emits [`SETTINGS_EVENT`] to the webview, which raises its `openSettings` command. In
//! the app the ⌘, accelerator belongs to this menu item only (the webview's own ⌘, handler is off
//! under Tauri), so the shortcut cannot fire twice.

use tauri::menu::{
    AboutMetadata, CheckMenuItem, HELP_SUBMENU_ID, Menu, MenuEvent, MenuItem, MenuItemKind,
    PredefinedMenuItem, Submenu, WINDOW_SUBMENU_ID,
};
use tauri::{AppHandle, Emitter, Runtime};

/// Menu item id of "Settings…".
pub const SETTINGS_ITEM_ID: &str = "lotse.settings";
/// Webview event raised by "Settings…" (no payload).
pub const SETTINGS_EVENT: &str = "lotse://menu/settings";
const SETTINGS_ACCELERATOR: &str = "CmdOrCtrl+,";
/// Menu item id of "View › Show Status Bar" (a checkmark item: the webview owns the state).
pub const STATUSBAR_ITEM_ID: &str = "lotse.statusbar";
/// Webview event raised by "Show Status Bar" (no payload).
pub const STATUSBAR_EVENT: &str = "lotse://menu/statusbar";
/// Menu item id of "View › Lock Top Bar in Full Screen".
pub const TOPBAR_LOCK_ITEM_ID: &str = "lotse.topbar-lock";
/// Webview event raised by "Lock Top Bar in Full Screen" (no payload).
pub const TOPBAR_LOCK_EVENT: &str = "lotse://menu/topbar-lock";
/// Menu item id of "View › Sidebar on Right".
pub const SIDEBAR_SIDE_ITEM_ID: &str = "lotse.sidebar-side";
/// Webview event raised by "Sidebar on Right" (no payload).
pub const SIDEBAR_SIDE_EVENT: &str = "lotse://menu/sidebar-side";
/// Id of the View submenu, so [`set_menu_check`] can find its items.
const VIEW_SUBMENU_ID: &str = "lotse.view";

/// The webview event a custom menu item raises, if it is one of ours.
pub fn event_for_item(id: &str) -> Option<&'static str> {
    match id {
        SETTINGS_ITEM_ID => Some(SETTINGS_EVENT),
        STATUSBAR_ITEM_ID => Some(STATUSBAR_EVENT),
        TOPBAR_LOCK_ITEM_ID => Some(TOPBAR_LOCK_EVENT),
        SIDEBAR_SIDE_ITEM_ID => Some(SIDEBAR_SIDE_EVENT),
        _ => None,
    }
}

/// Menu click handler (`App::on_menu_event`): forward our items to the webview.
pub fn on_menu_event<R: Runtime>(app: &AppHandle<R>, event: MenuEvent) {
    if let Some(name) = event_for_item(event.id().as_ref())
        && let Err(e) = app.emit(name, ())
    {
        tracing::warn!("emitting {name}: {e}");
    }
}

pub fn build<R: Runtime>(app: &AppHandle<R>) -> tauri::Result<Menu<R>> {
    let info = app.package_info();
    let about = AboutMetadata {
        name: Some(info.name.clone()),
        version: Some(info.version.to_string()),
        copyright: app.config().bundle.copyright.clone(),
        authors: app.config().bundle.publisher.clone().map(|p| vec![p]),
        ..Default::default()
    };
    let settings = MenuItem::with_id(
        app,
        SETTINGS_ITEM_ID,
        "Settings…",
        true,
        Some(SETTINGS_ACCELERATOR),
    )?;

    let app_menu = Submenu::with_items(
        app,
        info.name.clone(),
        true,
        &[
            &PredefinedMenuItem::about(app, None, Some(about))?,
            &PredefinedMenuItem::separator(app)?,
            &settings,
            &PredefinedMenuItem::separator(app)?,
            &PredefinedMenuItem::services(app, None)?,
            &PredefinedMenuItem::hide(app, None)?,
            &PredefinedMenuItem::hide_others(app, None)?,
            &PredefinedMenuItem::show_all(app, None)?,
            &PredefinedMenuItem::separator(app)?,
            &PredefinedMenuItem::quit(app, None)?,
        ],
    )?;
    let file = Submenu::with_items(
        app,
        "File",
        true,
        &[&PredefinedMenuItem::close_window(app, None)?],
    )?;
    let edit = Submenu::with_items(
        app,
        "Edit",
        true,
        &[
            &PredefinedMenuItem::undo(app, None)?,
            &PredefinedMenuItem::redo(app, None)?,
            &PredefinedMenuItem::separator(app)?,
            &PredefinedMenuItem::cut(app, None)?,
            &PredefinedMenuItem::copy(app, None)?,
            &PredefinedMenuItem::paste(app, None)?,
            &PredefinedMenuItem::select_all(app, None)?,
        ],
    )?;
    // No accelerators: a ⌘ chord taken by the menu never reaches the terminal. The webview owns each state (saved in
    // localStorage) and ticks the item through `set_menu_check`; a click toggles the tick natively and the webview
    // toggles its state on the event, so the two stay in step.
    let check = |id, text| CheckMenuItem::with_id(app, id, text, true, false, None::<&str>);
    let status_bar = check(STATUSBAR_ITEM_ID, "Show Status Bar")?;
    let topbar_lock = check(TOPBAR_LOCK_ITEM_ID, "Lock Top Bar in Full Screen")?;
    let sidebar_side = check(SIDEBAR_SIDE_ITEM_ID, "Sidebar on Right")?;
    let view = Submenu::with_id_and_items(
        app,
        VIEW_SUBMENU_ID,
        "View",
        true,
        &[
            &status_bar,
            &topbar_lock,
            &sidebar_side,
            &PredefinedMenuItem::separator(app)?,
            &PredefinedMenuItem::fullscreen(app, None)?,
        ],
    )?;
    // the ids are what lets macOS manage these as the Window and Help menus
    let window = Submenu::with_id_and_items(
        app,
        WINDOW_SUBMENU_ID,
        "Window",
        true,
        &[
            &PredefinedMenuItem::minimize(app, None)?,
            &PredefinedMenuItem::maximize(app, None)?,
            &PredefinedMenuItem::separator(app)?,
            &PredefinedMenuItem::close_window(app, None)?,
        ],
    )?;
    let help = Submenu::with_id_and_items(app, HELP_SUBMENU_ID, "Help", true, &[])?;

    Menu::with_items(app, &[&app_menu, &file, &edit, &view, &window, &help])
}

/// Tick or untick one of the View menu's checkmark items (the webview calls it when its saved choice loads or
/// changes). An unknown id is an error.
#[tauri::command]
pub fn set_menu_check<R: Runtime>(
    app: AppHandle<R>,
    id: String,
    checked: bool,
) -> Result<(), String> {
    let view = match app.menu().and_then(|m| m.get(VIEW_SUBMENU_ID)) {
        Some(MenuItemKind::Submenu(view)) => view,
        _ => return Err("no View menu".into()),
    };
    match view.get(&id) {
        Some(MenuItemKind::Check(item)) => item.set_checked(checked).map_err(|e| e.to_string()),
        _ => Err(format!("no checkmark item {id}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn settings_item_raises_the_settings_event_and_nothing_else_does() {
        assert_eq!(event_for_item(SETTINGS_ITEM_ID), Some(SETTINGS_EVENT));
        // predefined items (quit, copy, …) carry their own ids and are handled natively
        assert_eq!(event_for_item("quit"), None);
        assert_eq!(event_for_item(""), None);
    }

    #[test]
    fn status_bar_item_raises_its_own_event() {
        assert_eq!(event_for_item(STATUSBAR_ITEM_ID), Some(STATUSBAR_EVENT));
        assert!(STATUSBAR_ITEM_ID.starts_with("lotse."));
    }

    #[test]
    fn topbar_lock_item_raises_its_own_event() {
        assert_eq!(event_for_item(TOPBAR_LOCK_ITEM_ID), Some(TOPBAR_LOCK_EVENT));
    }

    #[test]
    fn sidebar_side_item_raises_its_own_event() {
        assert_eq!(
            event_for_item(SIDEBAR_SIDE_ITEM_ID),
            Some(SIDEBAR_SIDE_EVENT)
        );
    }

    #[test]
    fn settings_item_id_is_namespaced_and_the_accelerator_is_cmd_comma() {
        assert!(SETTINGS_ITEM_ID.starts_with("lotse."));
        assert_eq!(SETTINGS_ACCELERATOR, "CmdOrCtrl+,");
    }
}
