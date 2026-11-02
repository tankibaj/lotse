import type { SessionId } from '../lib/client';
import type { SplitDirection } from './layout';

// Lets other WPs (sidebar card ⌥-click, card context menu "Open in Split Right / Down", palette, tab drag) ask
// the panes to open a session next to the focused pane without importing the pane state. `Panes` subscribes while mounted.

type Handler = (sessionId: SessionId | null, direction: SplitDirection) => void;
const handlers = new Set<Handler>();

/** Panes side: subscribe; returns the unsubscribe function. */
export function onSplitRequest(handler: Handler): () => void {
  handlers.add(handler);
  return () => handlers.delete(handler);
}

/**
 * Open `sessionId` next to the focused pane: side by side (`right`, the default) or stacked below (`down`).
 * `null` picks the next session that is not open (what ⌘D / ⌘⇧D do). Returns the number of listeners that ran
 * (0: no panes mounted).
 */
export function openInSplit(sessionId: SessionId | null = null, direction: SplitDirection = 'right'): number {
  const copy = [...handlers];
  copy.forEach((h) => h(sessionId, direction));
  return copy.length;
}

/**
 * A tab dragged out of the tab strip over the panes (the tab strip drags with the pointer, not with HTML drag and
 * drop). `move`: show the drop zone under the pointer; `drop`: split the pane under the pointer; `cancel`: hide the zone.
 */
export type TabDrag =
  | { phase: 'move'; sessionId: SessionId; x: number; y: number }
  /** `restore`: the session that was selected before the mouse went down on the tab (that press selected the tab). */
  | { phase: 'drop'; sessionId: SessionId; x: number; y: number; restore: SessionId | null }
  | { phase: 'cancel' };
type DragHandler = (drag: TabDrag) => boolean;
const dragHandlers = new Set<DragHandler>();

/** Panes side: subscribe; the handler answers whether the pointer is over the panes. */
export function onTabDrag(handler: DragHandler): () => void {
  dragHandlers.add(handler);
  return () => dragHandlers.delete(handler);
}

/** Tab strip side: report the drag; true when the panes claim the pointer (so the strip stops reordering). */
export function dragTabOverPanes(drag: TabDrag): boolean {
  let over = false;
  [...dragHandlers].forEach((h) => {
    over = h(drag) || over;
  });
  return over;
}
