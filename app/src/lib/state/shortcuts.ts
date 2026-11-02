import { useEffect } from 'react';
import { useClient } from '../client/context';
import { getSelectedSessionId, selectSession } from '../ui/selection';
import { togglePausedNotifications } from './actions';
import { nextNeedYou } from './selectors';
import { useWorkspaceStore } from './store';

/**
 * Global session shortcuts (mounted by the status bar, which is always on screen, so they work with
 * the sidebar hidden): ⌘J next session that needs you, ⌘1…9 nth session of the current project,
 * ⌥⌘N pause notifications.
 */
export function useSessionShortcuts(): void {
  const client = useClient();
  const store = useWorkspaceStore();
  useEffect(() => {
    // Keep the store (snapshot and events) running even when no other component subscribes.
    const off = store.subscribe(() => {});
    const onKey = (e: KeyboardEvent) => {
      if (!e.metaKey || e.ctrlKey) return;
      const { sessions } = store.getState();
      // ⌘J matches the typed letter (Dvorak/Colemak safe); ⌥⌘N the physical key, since ⌥ changes the typed
      // character on macOS. Unverified on non-QWERTY layouts: check in the built app.
      if (e.key.toLowerCase() === 'j' && !e.shiftKey && !e.altKey) {
        e.preventDefault();
        const id = nextNeedYou(sessions, getSelectedSessionId());
        if (id) selectSession(id);
        return;
      }
      if (e.code === 'KeyN' && e.altKey && !e.shiftKey) {
        e.preventDefault();
        togglePausedNotifications(client).catch(() => {});
        return;
      }
      const digit = /^Digit([1-9])$/.exec(e.code);
      if (digit && !e.shiftKey && !e.altKey) {
        const current = sessions.find((s) => s.id === getSelectedSessionId());
        const target = sessions.filter((s) => s.project_id === current?.project_id)[Number(digit[1]) - 1];
        if (target) {
          e.preventDefault();
          selectSession(target.id);
        }
      }
    };
    addEventListener('keydown', onKey);
    return () => {
      removeEventListener('keydown', onKey);
      off();
    };
  }, [client, store]);
}
