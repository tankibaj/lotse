import { useSyncExternalStore } from 'react';
import type { SessionId } from '../client/types';

// The one shared "current session" (the mock's `cur`). Written by the sidebar (WP3: click, ⌘J, ⌘1…9)
// and the palette (WP5: jump to session); read by tabs/panes/terminal (WP4) and the status bar (WP3).
// It holds only the id: everything else comes from the client's snapshot and events.
// WP3 selects the initial session once the snapshot is loaded (`null` until then).

let selected: SessionId | null = null;
const listeners = new Set<() => void>();

export const getSelectedSessionId = (): SessionId | null => selected;

export function selectSession(id: SessionId | null): void {
  if (id === selected) return;
  selected = id;
  for (const l of [...listeners]) l();
}

export function subscribeSelection(listener: () => void): () => void {
  listeners.add(listener);
  return () => listeners.delete(listener);
}

/** React hook: re-renders when the selected session changes. */
export function useSelectedSessionId(): SessionId | null {
  return useSyncExternalStore(subscribeSelection, getSelectedSessionId);
}
