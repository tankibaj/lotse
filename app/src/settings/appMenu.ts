import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import { inTauri } from '../sheets/hostBridge';

/** Emitted by the app menu's "Settings…" item (`app/src-tauri/src/menu.rs`). */
export const SETTINGS_MENU_EVENT = 'lotse://menu/settings';

/** Emitted by the View menu's "Show Status Bar" item. */
export const STATUSBAR_MENU_EVENT = 'lotse://menu/statusbar';

/** Emitted by the View menu's "Lock Top Bar in Full Screen" item. */
export const TOPBAR_LOCK_MENU_EVENT = 'lotse://menu/topbar-lock';

/** Emitted by the View menu's "Sidebar on Right" item. */
export const SIDEBAR_SIDE_MENU_EVENT = 'lotse://menu/sidebar-side';

/**
 * Call `cb` when "Settings…" is chosen in the macOS app menu. In the app the menu owns ⌘, (its
 * accelerator); the browser build has no menu and returns a no-op. Returns the cleanup.
 */
export const onMenuSettings = (cb: () => void): (() => void) => onMenuEvent(SETTINGS_MENU_EVENT, cb);

/** Same for View › Show Status Bar. */
export const onMenuStatusBar = (cb: () => void): (() => void) => onMenuEvent(STATUSBAR_MENU_EVENT, cb);

/** Same for View › Lock Top Bar in Full Screen. */
export const onMenuTopBarLock = (cb: () => void): (() => void) => onMenuEvent(TOPBAR_LOCK_MENU_EVENT, cb);

/** Same for View › Sidebar on Right. */
export const onMenuSidebarSide = (cb: () => void): (() => void) => onMenuEvent(SIDEBAR_SIDE_MENU_EVENT, cb);

/** Menu item ids of the View menu's checkable items (`menu.rs`). */
export const STATUSBAR_ITEM_ID = 'lotse.statusbar';
export const TOPBAR_LOCK_ITEM_ID = 'lotse.topbar-lock';
export const SIDEBAR_SIDE_ITEM_ID = 'lotse.sidebar-side';

/** Tick or untick a View menu item (no-op in the browser, which has no menu). */
export function syncMenuCheck(id: string, checked: boolean): void {
  if (!inTauri()) return;
  invoke('set_menu_check', { id, checked }).catch(() => {});
}

function onMenuEvent(event: string, cb: () => void): () => void {
  if (!inTauri()) return () => {};
  let off: (() => void) | undefined;
  let stopped = false;
  listen(event, cb)
    .then((un) => {
      if (stopped) un();
      else off = un;
    })
    .catch(() => {});
  return () => {
    stopped = true;
    off?.();
  };
}
