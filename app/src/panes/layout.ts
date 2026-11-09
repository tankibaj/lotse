import type { SessionId, Status } from '../lib/client';

// Pane layout: up to four terminals in columns side by side, each column holding one or more panes stacked
// top to bottom (a two-level tree), one pane focused. Pure functions so add/close/focus/resize are testable
// without a DOM. Behaviour follows spec/mock.html (`panes`, `focusI`, `paneW`, `syncPanes`, `splitWith`,
// `closePane`, `dragDiv`).

/** Panes in total, however they are split. */
export const MAX_PANES = 4;
/** Panes in total on a display this wide (logical points) or wider. */
export const MAX_PANES_WIDE = 6;
export const WIDE_DISPLAY_PT = 3000;
/** Width of a column collapsed to a rail. */
export const RAIL_PX = 28;

/** Panes allowed on a display `widthPt` logical points wide. */
export const maxPanesFor = (widthPt: number): number => (widthPt >= WIDE_DISPLAY_PT ? MAX_PANES_WIDE : MAX_PANES);
/** Narrowest a pane may be dragged to (mock: 220 px). */
export const MIN_PANE_PX = 220;
/** Shortest a stacked pane may be dragged to: its header and a few terminal rows. */
export const MIN_PANE_HEIGHT_PX = 120;

export type SplitDirection = 'right' | 'down';

/** A file open in the editor, beside the terminal of the session it belongs to (`path` is relative to that session's folder). */
export interface EditorPane {
  kind: 'editor';
  sessionId: SessionId;
  path: string;
}

/** What a pane shows: a session's terminal (the bare session id) or an editor. */
export type PaneRef = SessionId | EditorPane;

export const isEditor = (p: PaneRef): p is EditorPane => typeof p !== 'string';
/** The session a pane belongs to. */
export const sessionOf = (p: PaneRef): SessionId => (typeof p === 'string' ? p : p.sessionId);
/** Key of a pane in `weights` / `heights` and in the DOM (`data-pane`): the session id, or `f:<session>:<path>` for an editor. */
export const paneKey = (p: PaneRef): string => (typeof p === 'string' ? p : `f:${p.sessionId}:${p.path}`);
/** The terminals among the panes, in order. */
export const sessionPanes = (l: PaneLayout): SessionId[] => l.panes.filter((p): p is SessionId => typeof p === 'string');

/** `open`: shown. `rail`: a 28 px strip (project tile, status) that restores on click. `hidden`: not shown until its session is selected. Either way the session keeps running and its panes stay in `panes`. */
export type ColumnMode = 'open' | 'rail' | 'hidden';

export interface PaneLayout {
  /** What each pane shows, in reading order: column by column, top to bottom. */
  panes: PaneRef[];
  /** Index of the focused pane. */
  focus: number;
  /** Relative width (flex-grow) per column, stored under the `paneKey` of the column's top pane (missing = 1). */
  weights: Record<string, number>;
  /** Panes per column, left to right; sums to `panes.length`. Missing: one pane per column (a plain row, the shape before split down). */
  cols?: number[];
  /** Relative height per pane inside a stacked column, by `paneKey` (missing = 1). */
  heights?: Record<string, number>;
  /** Columns that are not open, stored under the `paneKey` of the column's top pane (missing = open). The focused pane is always in an open column. */
  collapsed?: Record<string, Exclude<ColumnMode, 'open'>>;
}

export const emptyLayout = (): PaneLayout => ({ panes: [], focus: 0, weights: {} });

export const weightOf = (l: PaneLayout, key: string): number => l.weights[key] ?? 1;
export const heightOf = (l: PaneLayout, key: string): number => l.heights?.[key] ?? 1;

const validCols = (cols: unknown, count: number): cols is number[] =>
  Array.isArray(cols) && cols.length > 0 && cols.every((n) => Number.isInteger(n) && n > 0) && cols.reduce((a: number, n: number) => a + n, 0) === count;

/** The panes grouped by column, each column top to bottom. */
export function columnsOf(l: PaneLayout): PaneRef[][] {
  const sizes = validCols(l.cols, l.panes.length) ? l.cols : l.panes.map(() => 1);
  let at = 0;
  return sizes.map((n) => l.panes.slice(at, (at += n)));
}

/** Column and row of flat pane `index`, plus the flat index of its column's first pane. */
function locate(columns: readonly PaneRef[][], index: number): { col: number; row: number; start: number } {
  let start = 0;
  for (const [col, c] of columns.entries()) {
    if (index < start + c.length) return { col, row: index - start, start };
    start += c.length;
  }
  return { col: columns.length, row: 0, start };
}

const pick = <T>(from: Record<string, T> | undefined, keep: ReadonlySet<string>): Record<string, T> =>
  Object.fromEntries(Object.entries(from ?? {}).filter(([id]) => keep.has(id)));

/**
 * Assemble a layout from columns; the canonical shape omits `cols` for a plain row, `heights` when nothing is stacked or
 * resized, and `collapsed` when every column is open. A focused pane in a collapsed column reopens that column.
 */
function build(
  columns: PaneRef[][],
  focus: number,
  weights: Record<string, number>,
  heights?: Record<string, number>,
  collapsed?: PaneLayout['collapsed'],
): PaneLayout {
  const panes = columns.flat();
  const layout: PaneLayout = { panes, focus, weights: pick(weights, new Set(panes.map(paneKey))) };
  if (columns.some((c) => c.length > 1)) layout.cols = columns.map((c) => c.length);
  const stackedIds = new Set(columns.filter((c) => c.length > 1).flat().map(paneKey));
  const kept = pick(heights, stackedIds);
  if (Object.keys(kept).length > 0) layout.heights = kept;
  const tops = new Set(columns.map((c) => paneKey(c[0] as PaneRef)));
  const mode = pick(collapsed, tops);
  const focusedTop = columns[locate(columns, focus).col]?.[0];
  if (focusedTop !== undefined) delete mode[paneKey(focusedTop)];
  if (Object.keys(mode).length > 0) layout.collapsed = mode;
  return layout;
}

/** A per-column value keyed by top pane (width, collapse mode): when that pane leaves, the next one inherits it. */
function carryTops<T>(old: PaneLayout, columns: readonly PaneRef[][], from: Record<string, T> | undefined): Record<string, T> {
  const oldColumns = columnsOf(old);
  const out: Record<string, T> = {};
  for (const c of columns) {
    const top = paneKey(c[0] as PaneRef);
    const v = from?.[top] ?? (() => {
      const before = oldColumns.find((o) => o.some((p) => paneKey(p) === top));
      return before && paneKey(before[0] as PaneRef) !== top ? from?.[paneKey(before[0] as PaneRef)] : undefined;
    })();
    if (v !== undefined) out[top] = v;
  }
  return out;
}

const carryWidths = (old: PaneLayout, columns: readonly PaneRef[][]) => carryTops(old, columns, old.weights);
const carryCollapsed = (old: PaneLayout, columns: readonly PaneRef[][]) => carryTops(old, columns, old.collapsed);

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
  const numbers = (v: unknown): Record<string, number> =>
    typeof v === 'object' && v !== null
      ? Object.fromEntries(Object.entries(v).filter((e): e is [string, number] => typeof e[1] === 'number' && Number.isFinite(e[1]) && e[1] > 0))
      : {};
  const focus = typeof r.focus === 'number' && Number.isInteger(r.focus) ? Math.min(Math.max(0, r.focus), panes.length - 1) : 0;
  const collapsed =
    typeof r.collapsed === 'object' && r.collapsed !== null
      ? Object.fromEntries(Object.entries(r.collapsed).filter((e): e is [string, 'rail' | 'hidden'] => e[1] === 'rail' || e[1] === 'hidden'))
      : undefined;
  return build(columns, focus, numbers(r.weights), numbers(r.heights), collapsed);
}

/** Which way a drop at `x`,`y` inside a `width` x `height` pane splits it: the bottom half down, the right half right; where both apply, the nearer edge wins; the top-left quarter keeps the old default, right. */
export function dropDirection(x: number, y: number, width: number, height: number): SplitDirection {
  if (width <= 0 || height <= 0) return 'right';
  const fx = x / width;
  const fy = y / height;
  return fy > 0.5 && fy >= fx ? 'down' : 'right';
}

export type AddResult = { layout: PaneLayout; outcome: 'added' | 'focused' | 'full' | 'replaced' };

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
 * column, `down` below the focused pane in its column. Already open: focus it (its column reopens). `max` panes open: unchanged, `full`.
 */
export function addPane(l: PaneLayout, id: SessionId, direction: SplitDirection = 'right', max = MAX_PANES): AddResult {
  const open = l.panes.indexOf(id);
  if (open >= 0) return { layout: focusPane(l, open), outcome: 'focused' };
  if (l.panes.length >= max) return { layout: l, outcome: 'full' };
  const columns = columnsOf(l).map((c) => [...c]);
  const { col, row } = locate(columns, l.focus);
  const here = columns[col];
  if (here === undefined) columns.push([id]);
  else if (direction === 'down') here.splice(row + 1, 0, id);
  else columns.splice(col + 1, 0, [id]);
  return { layout: build(columns, columns.flat().indexOf(id), l.weights, l.heights, carryCollapsed(l, columns)), outcome: 'added' };
}

/**
 * Open `path` of `sessionId` in an editor column right of that session's terminal (right of the focused pane when the
 * terminal is not open, or when an editor has focus). Already open: focus it. `max` panes open: the last editor is
 * replaced in place; with no editor to replace, unchanged, `full`. A terminal is never replaced.
 */
export function addEditor(l: PaneLayout, sessionId: SessionId, path: string, max = MAX_PANES): AddResult {
  const editor: EditorPane = { kind: 'editor', sessionId, path };
  const key = paneKey(editor);
  const open = l.panes.findIndex((p) => paneKey(p) === key);
  if (open >= 0) return { layout: focusPane(l, open), outcome: 'focused' };
  if (l.panes.length >= max) {
    const last = l.panes.map(isEditor).lastIndexOf(true);
    if (last < 0) return { layout: l, outcome: 'full' };
    const old = paneKey(l.panes[last] as PaneRef);
    const columns = columnsOf(l).map((c) => c.map((p) => (paneKey(p) === old ? editor : p)));
    const carry = (from: Record<string, number> | undefined) => (from && old in from ? { ...from, [key]: from[old] as number } : from);
    return { layout: build(columns, last, carry(l.weights) ?? {}, carry(l.heights), carryCollapsed(l, columns)), outcome: 'replaced' };
  }
  const columns = columnsOf(l).map((c) => [...c]);
  const anchor = isEditor(l.panes[l.focus] as PaneRef) ? l.focus : l.panes.indexOf(sessionId) >= 0 ? l.panes.indexOf(sessionId) : l.focus;
  const { col } = locate(columns, anchor);
  columns.splice(col + 1, 0, [editor]);
  return { layout: build(columns, columns.flat().findIndex((p) => paneKey(p) === key), l.weights, l.heights, carryCollapsed(l, columns)), outcome: 'added' };
}

/**
 * Remove pane `index` (the session keeps running); its column closes with its last pane. The last pane, and the last
 * terminal, cannot be closed. Focus stays on the same session where possible; closing the focused pane moves it to the pane above,
 * else the one that takes its place, else the left neighbour.
 */
export function closePane(l: PaneLayout, index: number): PaneLayout {
  if (l.panes.length < 2 || index < 0 || index >= l.panes.length) return l;
  if (!isEditor(l.panes[index] as PaneRef) && sessionPanes(l).length < 2) return l; // the terminal stays while an editor is open
  const before = columnsOf(l);
  const { col, row } = locate(before, index);
  const size = before[col]?.length ?? 1;
  const columns = before.map((c, i) => (i === col ? c.filter((_, r) => r !== row) : c)).filter((c) => c.length > 0);
  let focus = l.focus;
  if (l.focus > index) focus = l.focus - 1;
  else if (l.focus === index) focus = size > 1 ? (row > 0 ? index - 1 : index) : Math.max(0, index - 1);
  return build(columns, Math.min(focus, l.panes.length - 2), carryWidths(l, columns), l.heights, carryCollapsed(l, columns));
}

/** Focus pane `index`; its column reopens when it was collapsed or hidden, so the focused pane is always visible. */
export function focusPane(l: PaneLayout, index: number): PaneLayout {
  if (index < 0 || index >= l.panes.length) return l;
  if (columnMode(l, index) !== 'open') return restoreColumn(l, index);
  return index === l.focus ? l : { ...l, focus: index };
}

/** ⌘[ (delta -1) and ⌘] (delta +1): move focus through the panes of open columns, wrapping around the ends. */
export function focusStep(l: PaneLayout, delta: number): PaneLayout {
  const open = l.panes.flatMap((_, i) => (columnMode(l, i) === 'open' ? [i] : []));
  const n = open.length;
  if (n < 2) return l;
  const at = Math.max(0, open.indexOf(l.focus));
  return focusPane(l, open[(((at + delta) % n) + n) % n] as number);
}

/** How the column holding flat pane `index` is shown. */
export function columnMode(l: PaneLayout, index: number): ColumnMode {
  const columns = columnsOf(l);
  const top = columns[locate(columns, index).col]?.[0];
  return (top !== undefined ? l.collapsed?.[paneKey(top)] : undefined) ?? 'open';
}

/** Collapse (to a rail) or hide the column holding pane `index`. At least one column stays open; focus leaves the column for the nearest open one (right first). */
function setColumnMode(l: PaneLayout, index: number, mode: 'rail' | 'hidden'): PaneLayout {
  const columns = columnsOf(l);
  const { col } = locate(columns, index);
  const here = columns[col];
  if (here === undefined) return l;
  const topKey = paneKey(here[0] as PaneRef);
  const starts = columns.map((_, c) => columns.slice(0, c).reduce((n, x) => n + x.length, 0));
  const isOpen = (c: number) => c !== col && columnMode(l, starts[c] as number) === 'open';
  const others = columns.map((_, c) => c).filter(isOpen);
  if (others.length === 0) return l;
  let focus = l.focus;
  if (locate(columns, l.focus).col === col) focus = starts[others.find((c) => c > col) ?? (others[others.length - 1] as number)] as number;
  return build(columns, focus, l.weights, l.heights, { ...l.collapsed, [topKey]: mode });
}

export const collapseColumn = (l: PaneLayout, index: number): PaneLayout => setColumnMode(l, index, 'rail');
export const hideColumn = (l: PaneLayout, index: number): PaneLayout => setColumnMode(l, index, 'hidden');

/** Open the column holding pane `index` again and focus that pane. */
export function restoreColumn(l: PaneLayout, index: number): PaneLayout {
  if (index < 0 || index >= l.panes.length) return l;
  const columns = columnsOf(l);
  const top = columns[locate(columns, index).col]?.[0];
  const collapsed = { ...l.collapsed };
  if (top !== undefined) delete collapsed[paneKey(top)];
  return build(columns, index, l.weights, l.heights, collapsed);
}

/** The panes of the column holding flat pane `index`, top to bottom. */
export function paneColumn(l: PaneLayout, index: number): PaneRef[] {
  const columns = columnsOf(l);
  return columns[locate(columns, index).col] ?? [];
}

/** Drop panes beyond `max` from the end (a layout saved for a wider display, say); the focused pane and the `keep` keys stay. */
export function limitPanes(l: PaneLayout, max: number, keep: ReadonlySet<string> = new Set()): PaneLayout {
  let out = l;
  for (let i = out.panes.length - 1; i >= 0 && out.panes.length > max; i--) {
    if (i === out.focus || keep.has(paneKey(out.panes[i] as PaneRef))) continue;
    out = closePane(out, i);
  }
  return out;
}

/** `restored` plus the panes of `current` named in `keep` (editors with unsaved edits), each as a column at the end; the focus stays where `restored` had it. */
export function keepPanes(restored: PaneLayout, current: PaneLayout, keep: ReadonlySet<string>): PaneLayout {
  const have = new Set(restored.panes.map(paneKey));
  const extra = current.panes.filter((p) => keep.has(paneKey(p)) && !have.has(paneKey(p)));
  if (extra.length === 0) return restored;
  const columns = [...columnsOf(restored), ...extra.map((p) => [p])];
  return build(columns, restored.focus, restored.weights, restored.heights, carryCollapsed(restored, columns));
}

/** The top panes of the two columns around the divider left of pane `index` (which must start a column after the first), or null. */
function columnPair(l: PaneLayout, index: number): [string, string] | null {
  const columns = columnsOf(l);
  const { col, row } = locate(columns, index);
  const a = columns[col - 1]?.[0];
  const b = columns[col]?.[0];
  return row === 0 && a !== undefined && b !== undefined ? [paneKey(a), paneKey(b)] : null;
}

/** The two panes around the divider above pane `index` (which must sit below another pane of its column), or null. */
function stackPair(l: PaneLayout, index: number): [string, string] | null {
  const { row } = locate(columnsOf(l), index);
  const a = l.panes[index - 1];
  const b = l.panes[index];
  return row > 0 && a !== undefined && b !== undefined ? [paneKey(a), paneKey(b)] : null;
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
 * drop sessions that are gone, and their editors; a selected session that is open takes focus (an editor of it that
 * has focus keeps it), otherwise it replaces the focused terminal (the nearest one when an editor has focus). No
 * sessions or nothing selected leaves an empty layout.
 */
export function syncLayout(l: PaneLayout, selected: SessionId | null, exists: (id: SessionId) => boolean): PaneLayout {
  let columns = columnsOf(l)
    .map((c) => c.filter((p) => exists(sessionOf(p))))
    .filter((c) => c.length > 0);
  let panes = columns.flat();
  let focus = l.focus;
  if (panes.length !== l.panes.length) focus = Math.min(focus, Math.max(0, panes.length - 1));
  if (selected !== null && exists(selected)) {
    const focused = panes[focus];
    const at = panes.indexOf(selected);
    if (focused !== undefined && sessionOf(focused) === selected) {
      // already on this session (its terminal or one of its editors)
    } else if (at >= 0) focus = at;
    else if (panes.length === 0) {
      columns = [[selected]];
      focus = 0;
    } else {
      const terminals = panes.flatMap((p, i) => (isEditor(p) ? [] : [i]));
      const target = terminals.filter((i) => i <= focus).pop() ?? terminals[0] ?? focus;
      const replaced = panes[target];
      columns = columns.map((c) => c.map((p) => (p === replaced ? selected : p)));
      focus = target;
    }
    panes = columns.flat();
  }
  if (panes.length === l.panes.length && focus === l.focus && panes.every((p, i) => paneKey(p) === paneKey(l.panes[i] as PaneRef))) return l;
  return build(columns, focus, carryWidths(l, columns), l.heights, carryCollapsed(l, columns));
}

/** Name of a display the window could not name (no monitor lookup, or a monitor without a name). */
export const DISPLAY_FALLBACK_NAME = 'display';

/** A display, as the window sees it: its name and size in logical points, plus scale and position when known. */
export interface Display {
  name: string;
  width: number;
  height: number;
  scale?: number;
  x?: number;
  y?: number;
}

/**
 * Storage key of the layout for a display: name and size, the scale when it is not 1 (one monitor at two scales), and the
 * position when the display has no name of its own (two unnamed monitors).
 */
export function layoutKey(d: Display): string {
  const scale = d.scale !== undefined && d.scale !== 1 ? `@${d.scale}x` : '';
  const at = d.name === DISPLAY_FALLBACK_NAME && d.x !== undefined && d.y !== undefined ? `+${d.x},${d.y}` : '';
  return `lotse-panes:${d.name}:${d.width}x${d.height}${scale}${at}`;
}

/** The layout as stored: terminals only (an editor's file is not worth reopening on its own). */
function withoutEditors(l: PaneLayout): PaneLayout {
  const columns = columnsOf(l).map((c) => c.filter((p) => !isEditor(p))).filter((c) => c.length > 0);
  const focused = l.panes[l.focus];
  const focus = Math.max(0, columns.flat().findIndex((p) => focused !== undefined && sessionOf(p) === sessionOf(focused)));
  return build(columns, focus, carryWidths(l, columns), l.heights, carryCollapsed(l, columns));
}

export function saveLayout(storage: Pick<Storage, 'setItem'>, key: string, l: PaneLayout): void {
  const stored = withoutEditors(l);
  if (stored.panes.length === 0) return;
  try {
    storage.setItem(key, JSON.stringify(stored));
  } catch {
    // storage full or unavailable: the layout just is not remembered
  }
}

/** The layout stored under `key`, or null when there is none or it is unreadable. */
export function loadLayout(storage: Pick<Storage, 'getItem'>, key: string): PaneLayout | null {
  try {
    const raw = storage.getItem(key);
    if (raw === null) return null;
    const l = migrateLayout(JSON.parse(raw));
    return l.panes.length > 0 ? l : null;
  } catch {
    return null;
  }
}
