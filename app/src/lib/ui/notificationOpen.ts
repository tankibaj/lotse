import { listen } from '@tauri-apps/api/event';
import { inTauri } from '../../sheets/hostBridge';
import { selectSession } from './selection';

/** Emitted when a notification body is clicked (`app/src-tauri/src/un.rs`); the payload is the session id. */
export const OPEN_SESSION_EVENT = 'lotse://notification/open';

/**
 * Select the session of a clicked notification (the window is already in front). The browser build
 * has no notifications and listens to nothing. Returns the cleanup.
 */
export function installNotificationOpen(): () => void {
  if (!inTauri()) return () => {};
  let off: (() => void) | undefined;
  let stopped = false;
  listen<unknown>(OPEN_SESSION_EVENT, (e) => {
    if (typeof e.payload === 'string' && e.payload) selectSession(e.payload);
  })
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
