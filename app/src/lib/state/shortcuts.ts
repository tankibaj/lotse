import { useEffect } from 'react';
import { useClient } from '../client/context';
import { getSelectedSessionId, selectSession } from '../ui/selection';
import { projectTabs } from '../ui/tabOrder';
import { startSession, togglePausedNotifications } from './actions';
import { nextNeedYou } from './selectors';
import { useWorkspaceStore } from './store';

/**
 * Global session shortcuts (mounted by the status bar, which is always on screen, so they work with
 * the sidebar hidden): ⌘J next session that needs you, ⌘1…9 nth tab of the current project,
 * ⌥⌘N pause notifications, ⌘T new terminal, ⌥⌘T new Claude Code session.
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
      // ⌘T a new terminal, ⌥⌘T a new Claude Code session, in the current project (physical key, like ⌥⌘N).
      if (e.code === 'KeyT' && !e.shiftKey) {
        e.preventDefault();
        startSession(client, store.getState(), e.altKey ? 'claude_code' : 'terminal').catch(() => {});
        return;
      }
      const digit = /^Digit([1-9])$/.exec(e.code);
      if (digit && !e.shiftKey && !e.altKey) {
        const current = sessions.find((s) => s.id === getSelectedSessionId());
        const target = projectTabs(sessions, current?.project_id)[Number(digit[1]) - 1];
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
