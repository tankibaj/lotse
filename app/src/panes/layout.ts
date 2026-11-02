import type { SessionId, Status } from '../lib/client';

// Pane layout: up to four terminals in columns side by side, each column holding one or more panes stacked
// top to bottom (a two-level tree), one pane focused. Pure functions so add/close/focus/resize are testable
// without a DOM. Behaviour follows spec/mock.html (`panes`, `focusI`, `paneW`, `syncPanes`, `splitWith`,
// `closePane`, `dragDiv`).

/** Panes in total, however they are split. */
export const MAX_PANES = 4;
/** Narrowest a pane may be dragged to (mock: 220 px). */
export const MIN_PANE_PX = 220;
/** Shortest a stacked pane may be dragged to: its header and a few terminal rows. */
export const MIN_PANE_HEIGHT_PX = 120;

export type SplitDirection = 'right' | 'down';

export interface PaneLayout {
  /** Session shown in each pane, in reading order: column by column, top to bottom. */
  panes: SessionId[];
  /** Index of the focused pane. */
  focus: number;
  /** Relative width (flex-grow) per column, stored under the session of the column's top pane (missing = 1). */
  weights: Record<SessionId, number>;
  /** Panes per column, left to right; sums to `panes.length`. Missing: one pane per column (a plain row, the shape before split down). */
  cols?: number[];
  /** Relative height per pane inside a stacked column (missing = 1). */
  heights?: Record<SessionId, number>;
}

export const emptyLayout = (): PaneLayout => ({ panes: [], focus: 0, weights: {} });

export const weightOf = (l: PaneLayout, id: SessionId): number => l.weights[id] ?? 1;
export const heightOf = (l: PaneLayout, id: SessionId): number => l.heights?.[id] ?? 1;

const validCols = (cols: unknown, count: number): cols is number[] =>
  Array.isArray(cols) && cols.length > 0 && cols.every((n) => Number.isInteger(n) && n > 0) && cols.reduce((a: number, n: number) => a + n, 0) === count;

/** The panes grouped by column, each column top to bottom. */
export function columnsOf(l: PaneLayout): SessionId[][] {
  const sizes = validCols(l.cols, l.panes.length) ? l.cols : l.panes.map(() => 1);
  let at = 0;
  return sizes.map((n) => l.panes.slice(at, (at += n)));
}

/** Column and row of flat pane `index`, plus the flat index of its column's first pane. */
function locate(columns: readonly SessionId[][], index: number): { col: number; row: number; start: number } {
  let start = 0;
  for (const [col, c] of columns.entries()) {
    if (index < start + c.length) return { col, row: index - start, start };
    start += c.length;
  }
  return { col: columns.length, row: 0, start };
}

const pick = (from: Record<SessionId, number> | undefined, keep: ReadonlySet<SessionId>): Record<SessionId, number> =>
  Object.fromEntries(Object.entries(from ?? {}).filter(([id]) => keep.has(id)));

/** Assemble a layout from columns; the canonical shape omits `cols` for a plain row and `heights` when nothing is stacked or resized. */
function build(columns: SessionId[][], focus: number, weights: Record<SessionId, number>, heights?: Record<SessionId, number>): PaneLayout {
  const panes = columns.flat();
  const layout: PaneLayout = { panes, focus, weights: pick(weights, new Set(panes)) };
  if (columns.some((c) => c.length > 1)) layout.cols = columns.map((c) => c.length);
  const stackedIds = new Set(columns.filter((c) => c.length > 1).flat());
  const kept = pick(heights, stackedIds);
  if (Object.keys(kept).length > 0) layout.heights = kept;
  return layout;
}

/** Column widths follow the top pane of each column; when that pane leaves, the next one inherits the width. */
function carryWidths(old: PaneLayout, columns: readonly SessionId[][]): Record<SessionId, number> {
  const oldColumns = columnsOf(old);
  const out: Record<SessionId, number> = {};
  for (const c of columns) {
    const top = c[0] as SessionId;
    const w = old.weights[top] ?? (() => {
      const before = oldColumns.find((o) => o.includes(top));
      return before && before[0] !== top ? old.weights[before[0] as SessionId] : undefined;
    })();
    if (w !== undefined) out[top] = w;
  }
  return out;
}

/** Rebuild a layout that arrives as plain data (e.g. read from storage): any shape, including the row-only one from before split down. Anything unusable falls back to a plain row, or to the empty layout. */
export function migrateLayout(raw: unknown): PaneLayout {
  if (typeof raw !== 'object' || raw === null) return emptyLayout();
  const r = raw as Record<string, unknown>;
  if (!Array.isArray(r.panes)) return emptyLayout();
  const panes = [...new Set((r.panes as unknown[]).filter((p): p is string => typeof p === 'string'))];
  if (panes.length === 0) return emptyLayout();
  const sizes = panes.length === r.panes.length && validCols(r.cols, panes.length) ? r.cols : panes.map(() => 1);
  let at = 0;
  const columns = sizes.map((n) => panes.slice(at, (at += n)));
  const numbers = (v: unknown): Record<SessionId, number> =>
    typeof v === 'object' && v !== null
      ? Object.fromEntries(Object.entries(v).filter((e): e is [string, number] => typeof e[1] === 'number' && Number.isFinite(e[1]) && e[1] > 0))
      : {};
  const focus = typeof r.focus === 'number' && Number.isInteger(r.focus) ? Math.min(Math.max(0, r.focus), panes.length - 1) : 0;
  return build(columns, focus, numbers(r.weights), numbers(r.heights));
}

/** Which way a drop at `x`,`y` inside a `width` x `height` pane splits it: the bottom half down, the right half right; where both apply, the nearer edge wins; the top-left quarter keeps the old default, right. */
export function dropDirection(x: number, y: number, width: number, height: number): SplitDirection {
  if (width <= 0 || height <= 0) return 'right';
  const fx = x / width;
  const fy = y / height;
  return fy > 0.5 && fy >= fx ? 'down' : 'right';
}

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

/**
 * Open `id` next to the focused pane: `right` (default) as a new full-height column after the focused pane's
 * column, `down` below the focused pane in its column. Already open: focus it. Four panes open: unchanged, `full`.
 */
export function addPane(l: PaneLayout, id: SessionId, direction: SplitDirection = 'right'): AddResult {
  const open = l.panes.indexOf(id);
  if (open >= 0) return { layout: focusPane(l, open), outcome: 'focused' };
  if (l.panes.length >= MAX_PANES) return { layout: l, outcome: 'full' };
  const columns = columnsOf(l).map((c) => [...c]);
  const { col, row } = locate(columns, l.focus);
  const here = columns[col];
  if (here === undefined) columns.push([id]);
  else if (direction === 'down') here.splice(row + 1, 0, id);
  else columns.splice(col + 1, 0, [id]);
  return { layout: build(columns, columns.flat().indexOf(id), l.weights, l.heights), outcome: 'added' };
}

/**
 * Remove pane `index` (the session keeps running); its column closes with its last pane. The last pane cannot be
 * closed. Focus stays on the same session where possible; closing the focused pane moves it to the pane above,
 * else the one that takes its place, else the left neighbour.
 */
export function closePane(l: PaneLayout, index: number): PaneLayout {
  if (l.panes.length < 2 || index < 0 || index >= l.panes.length) return l;
  const before = columnsOf(l);
  const { col, row } = locate(before, index);
  const size = before[col]?.length ?? 1;
  const columns = before.map((c, i) => (i === col ? c.filter((_, r) => r !== row) : c)).filter((c) => c.length > 0);
  let focus = l.focus;
  if (l.focus > index) focus = l.focus - 1;
  else if (l.focus === index) focus = size > 1 ? (row > 0 ? index - 1 : index) : Math.max(0, index - 1);
  return build(columns, Math.min(focus, l.panes.length - 2), carryWidths(l, columns), l.heights);
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

/** The top panes of the two columns around the divider left of pane `index` (which must start a column after the first), or null. */
function columnPair(l: PaneLayout, index: number): [SessionId, SessionId] | null {
  const columns = columnsOf(l);
  const { col, row } = locate(columns, index);
  const a = columns[col - 1]?.[0];
  const b = columns[col]?.[0];
  return row === 0 && a !== undefined && b !== undefined ? [a, b] : null;
}

/** The two panes around the divider above pane `index` (which must sit below another pane of its column), or null. */
function stackPair(l: PaneLayout, index: number): [SessionId, SessionId] | null {
  const { row } = locate(columnsOf(l), index);
  const a = l.panes[index - 1];
  const b = l.panes[index];
  return row > 0 && a !== undefined && b !== undefined ? [a, b] : null;
}

/**
 * Drag the divider left of pane `index` (between its column and the one before). `leftPx` is the wanted width of
 * the left column, `totalPx` the measured width of the pair. Both columns keep at least `minPx`.
 */
export function resizePair(l: PaneLayout, index: number, leftPx: number, totalPx: number, minPx = MIN_PANE_PX): PaneLayout {
  const pair = columnPair(l, index);
  if (pair === null || totalPx <= 2 * minPx) return l;
  const [a, b] = pair;
  const left = Math.min(totalPx - minPx, Math.max(minPx, leftPx));
  const sum = weightOf(l, a) + weightOf(l, b);
  return { ...l, weights: { ...l.weights, [a]: (sum * left) / totalPx, [b]: (sum * (totalPx - left)) / totalPx } };
}

/** Share of the pair's width (0-1) currently given to the left column. */
export function leftShare(l: PaneLayout, index: number): number {
  const pair = columnPair(l, index);
  if (pair === null) return 0.5;
  return weightOf(l, pair[0]) / (weightOf(l, pair[0]) + weightOf(l, pair[1]));
}

/** Like `resizePair` for the horizontal divider above stacked pane `index`: `topPx` is the wanted height of the upper pane. */
export function resizeStack(l: PaneLayout, index: number, topPx: number, totalPx: number, minPx = MIN_PANE_HEIGHT_PX): PaneLayout {
  const pair = stackPair(l, index);
  if (pair === null || totalPx <= 2 * minPx) return l;
  const [a, b] = pair;
  const top = Math.min(totalPx - minPx, Math.max(minPx, topPx));
  const sum = heightOf(l, a) + heightOf(l, b);
  return { ...l, heights: { ...l.heights, [a]: (sum * top) / totalPx, [b]: (sum * (totalPx - top)) / totalPx } };
}

/** Share of the pair's height (0-1) currently given to the upper pane. */
export function topShare(l: PaneLayout, index: number): number {
  const pair = stackPair(l, index);
  if (pair === null) return 0.5;
  return heightOf(l, pair[0]) / (heightOf(l, pair[0]) + heightOf(l, pair[1]));
}

/**
 * Keep the layout consistent with the selected session and the sessions that exist (mock `syncPanes`):
 * drop sessions that are gone; a selected session that is open takes focus, otherwise it replaces the
 * focused pane. No sessions or nothing selected leaves an empty layout.
 */
export function syncLayout(l: PaneLayout, selected: SessionId | null, exists: (id: SessionId) => boolean): PaneLayout {
  let columns = columnsOf(l)
    .map((c) => c.filter(exists))
    .filter((c) => c.length > 0);
  let panes = columns.flat();
  let focus = l.focus;
  if (panes.length !== l.panes.length) focus = Math.min(focus, Math.max(0, panes.length - 1));
  if (selected !== null && exists(selected)) {
    const at = panes.indexOf(selected);
    if (at >= 0) focus = at;
    else if (panes.length === 0) {
      columns = [[selected]];
      focus = 0;
    } else {
      const replaced = panes[focus];
      columns = columns.map((c) => c.map((p) => (p === replaced ? selected : p)));
    }
    panes = columns.flat();
  }
  if (panes.length === l.panes.length && focus === l.focus && panes.every((p, i) => p === l.panes[i])) return l;
  return build(columns, focus, carryWidths(l, columns), l.heights);
}
