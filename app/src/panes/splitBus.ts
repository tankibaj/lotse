import type { SessionId } from '../lib/client';

// Lets other WPs (sidebar card ⌥-click, card context menu "Open in Split Right", palette) ask the panes to
// open a session side by side without importing the pane state. `Panes` subscribes while mounted.

type Handler = (sessionId: SessionId | null) => void;
const handlers = new Set<Handler>();

/** Panes side: subscribe; returns the unsubscribe function. */
export function onSplitRequest(handler: Handler): () => void {
  handlers.add(handler);
  return () => handlers.delete(handler);
}

/**
 * Open `sessionId` side by side with the focused pane. `null` picks the next session that is not open
 * (what ⌘D does). Returns the number of listeners that ran (0: no panes mounted).
 */
export function openInSplit(sessionId: SessionId | null = null): number {
  const copy = [...handlers];
  copy.forEach((h) => h(sessionId));
  return copy.length;
}
