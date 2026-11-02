import { listen } from '@tauri-apps/api/event';
import { inTauri } from '../sheets/hostBridge';

/** Emitted by the app menu's "Settings…" item (`app/src-tauri/src/menu.rs`). */
export const SETTINGS_MENU_EVENT = 'lotse://menu/settings';

/**
 * Call `cb` when "Settings…" is chosen in the macOS app menu. In the app the menu owns ⌘, (its
 * accelerator); the browser build has no menu and returns a no-op. Returns the cleanup.
 */
export function onMenuSettings(cb: () => void): () => void {
  if (!inTauri()) return () => {};
  let off: (() => void) | undefined;
  let stopped = false;
  listen(SETTINGS_MENU_EVENT, cb)
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
