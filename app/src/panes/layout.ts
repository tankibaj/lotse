import type { SessionId, Status } from '../lib/client';

// Pane layout: up to four terminals side by side, one focused. Pure functions so add/close/focus/resize
// are testable without a DOM. Behaviour follows spec/mock.html (`panes`, `focusI`, `paneW`, `syncPanes`,
// `splitWith`, `closePane`, `dragDiv`).

export const MAX_PANES = 4;
/** Narrowest a pane may be dragged to (mock: 220 px). */
export const MIN_PANE_PX = 220;

export interface PaneLayout {
  /** Session shown in each pane, left to right. */
  panes: SessionId[];
  /** Index of the focused pane. */
  focus: number;
  /** Relative flex-grow per session (missing = 1). */
  weights: Record<SessionId, number>;
}

export const emptyLayout = (): PaneLayout => ({ panes: [], focus: 0, weights: {} });

export const weightOf = (l: PaneLayout, id: SessionId): number => l.weights[id] ?? 1;

export type AddResult = { layout: PaneLayout; outcome: 'added' | 'focused' | 'full' };

/** Needs-you first, as in the mock (`rank`): perm > fail > run > done > idle > unknown. */
const RANK: Record<Status, number> = { perm: 5, fail: 4, run: 3, done: 2, idle: 1, unknown: 0 };

/** The session ⌘D opens: the most urgent one that is not open yet (first in list order on ties), or null when all are open. */
export function nextSessionToOpen(sessions: ReadonlyArray<{ id: SessionId; status: Status }>, open: readonly SessionId[]): SessionId | null {
  let best: { id: SessionId; status: Status } | null = null;
  for (const s of sessions) {
    if (open.includes(s.id)) continue;
    if (best === null || RANK[s.status] > RANK[best.status]) best = s;
  }
  return best?.id ?? null;
}

/** Open `id` to the right of the focused pane. Already open: focus it. Four panes open: unchanged, `full`. */
export function addPane(l: PaneLayout, id: SessionId): AddResult {
  const open = l.panes.indexOf(id);
  if (open >= 0) return { layout: focusPane(l, open), outcome: 'focused' };
  if (l.panes.length >= MAX_PANES) return { layout: l, outcome: 'full' };
  const panes = [...l.panes];
  panes.splice(l.focus + 1, 0, id);
  return { layout: { ...l, panes, focus: l.focus + 1 }, outcome: 'added' };
}

/** Remove pane `index` (the session keeps running). The last pane cannot be closed. Focus stays on the same session where possible. */
export function closePane(l: PaneLayout, index: number): PaneLayout {
  if (l.panes.length < 2 || index < 0 || index >= l.panes.length) return l;
  const panes = l.panes.filter((_, i) => i !== index);
  const weights = { ...l.weights };
  delete weights[l.panes[index] as SessionId];
  const focus = Math.min(l.focus >= index ? Math.max(0, l.focus - 1) : l.focus, panes.length - 1);
  return { panes, focus, weights };
}

export function focusPane(l: PaneLayout, index: number): PaneLayout {
  if (index < 0 || index >= l.panes.length || index === l.focus) return l;
  return { ...l, focus: index };
}

/** ⌘[ (delta -1) and ⌘] (delta +1): move focus, wrapping around the ends. */
export function focusStep(l: PaneLayout, delta: number): PaneLayout {
  const n = l.panes.length;
  if (n < 2) return l;
  return focusPane(l, (((l.focus + delta) % n) + n) % n);
}

/**
 * Drag the divider left of pane `index` (between index-1 and index). `leftPx` is the wanted width of
 * the left pane, `totalPx` the measured width of the pair. Both panes keep at least `minPx`.
 */
export function resizePair(l: PaneLayout, index: number, leftPx: number, totalPx: number, minPx = MIN_PANE_PX): PaneLayout {
  const a = l.panes[index - 1];
  const b = l.panes[index];
  if (a === undefined || b === undefined || totalPx <= 2 * minPx) return l;
  const left = Math.min(totalPx - minPx, Math.max(minPx, leftPx));
  const sum = weightOf(l, a) + weightOf(l, b);
  return { ...l, weights: { ...l.weights, [a]: (sum * left) / totalPx, [b]: (sum * (totalPx - left)) / totalPx } };
}

/** Share of the pair's width (0-1) currently given to the left pane. */
export function leftShare(l: PaneLayout, index: number): number {
  const a = l.panes[index - 1];
  const b = l.panes[index];
  if (a === undefined || b === undefined) return 0.5;
  return weightOf(l, a) / (weightOf(l, a) + weightOf(l, b));
}

/**
 * Keep the layout consistent with the selected session and the sessions that exist (mock `syncPanes`):
 * drop sessions that are gone; a selected session that is open takes focus, otherwise it replaces the
 * focused pane. No sessions or nothing selected leaves an empty layout.
 */
export function syncLayout(l: PaneLayout, selected: SessionId | null, exists: (id: SessionId) => boolean): PaneLayout {
  let panes = l.panes.filter(exists);
  let focus = l.focus;
  if (panes.length !== l.panes.length) focus = Math.min(focus, Math.max(0, panes.length - 1));
  if (selected !== null && exists(selected)) {
    const at = panes.indexOf(selected);
    if (at >= 0) focus = at;
    else if (panes.length === 0) {
      panes = [selected];
      focus = 0;
    } else {
      panes = panes.map((p, i) => (i === focus ? selected : p));
    }
  }
  if (panes.length === l.panes.length && focus === l.focus && panes.every((p, i) => p === l.panes[i])) return l;
  const weights: Record<SessionId, number> = {};
  for (const p of panes) if (l.weights[p] !== undefined) weights[p] = l.weights[p];
  return { panes, focus, weights };
}
