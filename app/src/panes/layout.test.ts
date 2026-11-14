import { describe, expect, it } from 'vitest';
import {
  MAX_PANES,
  MIN_PANE_HEIGHT_PX,
  MIN_PANE_PX,
  addPane,
  closePane,
  columnsOf,
  dropDirection,
  emptyLayout,
  focusPane,
  focusStep,
  leftShare,
  migrateLayout,
  nextSessionToOpen,
  resizePair,
  resizeStack,
  syncLayout,
  topShare,
  weightOf,
} from './layout';
import type { PaneLayout } from './layout';

const layout = (panes: PaneLayout['panes'], focus = 0, weights: Record<string, number> = {}): PaneLayout => ({ panes, focus, weights });
/** Columns of stacked panes: stacked([['a1'], ['a2', 'a3']]) is a1 beside (a2 over a3). */
const stacked = (columns: string[][], focus = 0, extra: Partial<PaneLayout> = {}): PaneLayout => ({
  panes: columns.flat(),
  cols: columns.map((c) => c.length),
  focus,
  weights: {},
  ...extra,
});
const all = () => true;

describe('addPane', () => {
  it('opens the session right of the focused pane and focuses it', () => {
    const { layout: l, outcome } = addPane(layout(['a1', 'a2'], 0), 'a3');
    expect(outcome).toBe('added');
    expect(l.panes).toEqual(['a1', 'a3', 'a2']);
    expect(l.focus).toBe(1);
  });

  it('focuses a session that is already open instead of duplicating it', () => {
    const { layout: l, outcome } = addPane(layout(['a1', 'a2'], 0), 'a2');
    expect(outcome).toBe('focused');
    expect(l.panes).toEqual(['a1', 'a2']);
    expect(l.focus).toBe(1);
  });

  it(`stops at ${MAX_PANES} panes`, () => {
    const full = layout(['a1', 'a2', 'a3', 'a4'], 3);
    const { layout: l, outcome } = addPane(full, 'a5');
    expect(outcome).toBe('full');
    expect(l).toBe(full);
  });
});

describe('nextSessionToOpen', () => {
  const sessions = [
    { id: 'a1', status: 'run' as const },
    { id: 'a2', status: 'fail' as const },
    { id: 'a3', status: 'perm' as const },
    { id: 'a4', status: 'perm' as const },
  ];

  it('picks the most urgent session that is not open, first on ties', () => {
    expect(nextSessionToOpen(sessions, ['a1'])).toBe('a3');
    expect(nextSessionToOpen(sessions, ['a1', 'a3'])).toBe('a4');
    expect(nextSessionToOpen(sessions, ['a3', 'a4'])).toBe('a2');
  });

  it('returns null when every session is open', () => {
    expect(nextSessionToOpen(sessions, ['a1', 'a2', 'a3', 'a4'])).toBeNull();
    expect(nextSessionToOpen([], [])).toBeNull();
  });
});

describe('closePane', () => {
  it('removes the pane and its weight, focus stays on the same session', () => {
    const l = closePane(layout(['a1', 'a2', 'a3'], 2, { a1: 2, a2: 1, a3: 1 }), 0);
    expect(l.panes).toEqual(['a2', 'a3']);
    expect(l.focus).toBe(1);
    expect(l.weights).toEqual({ a2: 1, a3: 1 });
  });

  it('closing the focused pane moves focus to its left neighbour (or the new first pane)', () => {
    expect(closePane(layout(['a1', 'a2', 'a3'], 1), 1).focus).toBe(0);
    expect(closePane(layout(['a1', 'a2', 'a3'], 0), 0).focus).toBe(0);
  });

  it('never closes the last pane or an unknown index', () => {
    const one = layout(['a1']);
    expect(closePane(one, 0)).toBe(one);
    const two = layout(['a1', 'a2']);
    expect(closePane(two, 5)).toBe(two);
  });
});

describe('focus', () => {
  it('focusPane ignores out-of-range indexes', () => {
    const l = layout(['a1', 'a2'], 0);
    expect(focusPane(l, 1).focus).toBe(1);
    expect(focusPane(l, 2)).toBe(l);
    expect(focusPane(l, -1)).toBe(l);
  });

  it('focusStep (⌘[ and ⌘]) wraps around the ends', () => {
    const l = layout(['a1', 'a2', 'a3'], 0);
    expect(focusStep(l, -1).focus).toBe(2);
    expect(focusStep(layout(l.panes, 2), 1).focus).toBe(0);
    expect(focusStep(l, 1).focus).toBe(1);
  });

  it('focusStep does nothing with a single pane', () => {
    const one = layout(['a1']);
    expect(focusStep(one, 1)).toBe(one);
  });
});

describe('resizePair', () => {
  it('splits the pair weight by the dragged width and keeps the total', () => {
    const l = resizePair(layout(['a1', 'a2'], 0), 1, 300, 1000);
    expect(weightOf(l, 'a1')).toBeCloseTo(0.6);
    expect(weightOf(l, 'a2')).toBeCloseTo(1.4);
    expect(weightOf(l, 'a1') + weightOf(l, 'a2')).toBeCloseTo(2);
    expect(leftShare(l, 1)).toBeCloseTo(0.3);
  });

  it(`keeps both panes at least ${MIN_PANE_PX}px`, () => {
    const left = resizePair(layout(['a1', 'a2']), 1, 10, 1000);
    expect(leftShare(left, 1)).toBeCloseTo(MIN_PANE_PX / 1000);
    const right = resizePair(layout(['a1', 'a2']), 1, 990, 1000);
    expect(leftShare(right, 1)).toBeCloseTo((1000 - MIN_PANE_PX) / 1000);
  });

  it('only touches the two panes around the divider', () => {
    const l = resizePair(layout(['a1', 'a2', 'a3'], 0, { a1: 1, a2: 1, a3: 3 }), 1, 500, 1000);
    expect(weightOf(l, 'a3')).toBe(3);
    expect(leftShare(l, 1)).toBeCloseTo(0.5);
  });

  it('ignores a pair too narrow for two minimum panes, and a bad index', () => {
    const l = layout(['a1', 'a2']);
    expect(resizePair(l, 1, 200, 2 * MIN_PANE_PX)).toBe(l);
    expect(resizePair(l, 0, 300, 1000)).toBe(l);
    expect(resizePair(l, 2, 300, 1000)).toBe(l);
  });
});

describe('syncLayout', () => {
  it('starts from nothing with the selected session', () => {
    expect(syncLayout(emptyLayout(), 'a1', all)).toEqual(layout(['a1']));
  });

  it('stays empty until a session is selected', () => {
    const e = emptyLayout();
    expect(syncLayout(e, null, all)).toBe(e);
    expect(syncLayout(e, 'zz', (id) => id !== 'zz')).toBe(e);
  });

  it('selecting an open session focuses its pane', () => {
    expect(syncLayout(layout(['a1', 'a2', 'a3'], 0), 'a3', all).focus).toBe(2);
  });

  it('selecting a session that is not open replaces the focused pane', () => {
    const l = syncLayout(layout(['a1', 'a2'], 1, { a1: 2, a2: 3 }), 'a5', all);
    expect(l.panes).toEqual(['a1', 'a5']);
    expect(l.focus).toBe(1);
    expect(l.weights).toEqual({ a1: 2 });
  });

  it('drops sessions that no longer exist and clamps focus', () => {
    const l = syncLayout(layout(['a1', 'a2', 'a3'], 2), 'a1', (id) => id !== 'a3');
    expect(l.panes).toEqual(['a1', 'a2']);
    expect(l.focus).toBe(0);
  });

  it('is a no-op (same object) when nothing changes', () => {
    const l = layout(['a1', 'a2'], 1);
    expect(syncLayout(l, 'a2', all)).toBe(l);
  });

  it('keeps stacks intact and replaces the focused pane in place', () => {
    const l = syncLayout(stacked([['a1', 'a2'], ['a3']], 1), 'a9', all);
    expect(columnsOf(l)).toEqual([['a1', 'a9'], ['a3']]);
    expect(l.focus).toBe(1);
  });

  it('drops a vanished pane from its column and goes flat again when no stack is left', () => {
    const l = syncLayout(stacked([['a1', 'a2'], ['a3']], 0), 'a1', (id) => id !== 'a2');
    expect(columnsOf(l)).toEqual([['a1'], ['a3']]);
    expect(l.cols).toBeUndefined();
  });
});

describe('split down', () => {
  it('opens the session below the focused pane, in the same column, and focuses it', () => {
    const { layout: l, outcome } = addPane(layout(['a1', 'a2'], 0), 'a3', 'down');
    expect(outcome).toBe('added');
    expect(columnsOf(l)).toEqual([['a1', 'a3'], ['a2']]);
    expect(l.panes).toEqual(['a1', 'a3', 'a2']);
    expect(l.focus).toBe(1);
  });

  it('split right of a stacked pane opens a new full-height column after that column', () => {
    const { layout: l } = addPane(stacked([['a1', 'a2'], ['a3']], 0), 'a4', 'right');
    expect(columnsOf(l)).toEqual([['a1', 'a2'], ['a4'], ['a3']]);
    expect(l.panes[l.focus]).toBe('a4');
  });

  it('split down of the lower pane of a stack appends to it', () => {
    const { layout: l } = addPane(stacked([['a1', 'a2']], 1), 'a3', 'down');
    expect(columnsOf(l)).toEqual([['a1', 'a2', 'a3']]);
    expect(l.focus).toBe(2);
  });

  it('shares the pane limit with split right, and focuses a session that is already open', () => {
    const full = stacked([['a1', 'a2'], ['a3', 'a4']], 0);
    expect(addPane(full, 'a5', 'down')).toEqual({ layout: full, outcome: 'full' });
    const again = addPane(full, 'a4', 'down');
    expect(again.outcome).toBe('focused');
    expect(again.layout.focus).toBe(3);
  });

  it('a flat layout is a row of single-pane columns', () => {
    expect(columnsOf(layout(['a1', 'a2', 'a3']))).toEqual([['a1'], ['a2'], ['a3']]);
    expect(columnsOf(emptyLayout())).toEqual([]);
  });
});

describe('close in a split', () => {
  it('closing the lower pane of a stack collapses the stack and focuses the pane above', () => {
    const l = closePane(stacked([['a1'], ['a2', 'a3']], 2), 2);
    expect(columnsOf(l)).toEqual([['a1'], ['a2']]);
    expect(l.focus).toBe(1);
  });

  it('closing the upper pane of a stack focuses the pane that takes its place, and the column keeps its width', () => {
    const l = closePane(stacked([['a1'], ['a2', 'a3']], 1, { weights: { a1: 1, a2: 2.5 } }), 1);
    expect(columnsOf(l)).toEqual([['a1'], ['a3']]);
    expect(l.panes[l.focus]).toBe('a3');
    expect(weightOf(l, 'a3')).toBe(2.5);
    expect(l.weights.a2).toBeUndefined();
  });

  it('closing the only pane of a column removes the column', () => {
    const l = closePane(stacked([['a1', 'a2'], ['a3']], 2), 2);
    expect(columnsOf(l)).toEqual([['a1', 'a2']]);
    expect(l.cols).toEqual([2]);
    expect(l.focus).toBe(1);
  });

  it('forgets the height of a closed pane and goes back to the plain flat shape when no stack is left', () => {
    const l = closePane(stacked([['a1', 'a2']], 1, { heights: { a1: 0.6, a2: 1.4 } }), 1);
    expect(l.cols).toBeUndefined();
    expect(l.heights).toBeUndefined();
    expect(l.panes).toEqual(['a1']);
  });

  it('focus stays on the same session when an earlier pane closes', () => {
    const l = closePane(stacked([['a1', 'a2'], ['a3']], 2), 0);
    expect(l.panes).toEqual(['a2', 'a3']);
    expect(l.panes[l.focus]).toBe('a3');
  });
});

describe('migrateLayout', () => {
  it('treats a layout saved before split down (no cols) as a row of single-pane columns', () => {
    const l = migrateLayout({ panes: ['a1', 'a2', 'a3'], focus: 1, weights: { a1: 2 } });
    expect(columnsOf(l)).toEqual([['a1'], ['a2'], ['a3']]);
    expect(l).toMatchObject({ panes: ['a1', 'a2', 'a3'], focus: 1, weights: { a1: 2 } });
    expect(l.cols).toBeUndefined();
  });

  it('keeps a valid column shape', () => {
    const l = migrateLayout({ panes: ['a1', 'a2', 'a3'], cols: [1, 2], focus: 2, weights: {}, heights: { a2: 2 } });
    expect(columnsOf(l)).toEqual([['a1'], ['a2', 'a3']]);
    expect(l.heights).toEqual({ a2: 2 });
  });

  it('falls back to a row when cols does not add up, and drops duplicates, junk and an out-of-range focus', () => {
    expect(columnsOf(migrateLayout({ panes: ['a1', 'a2'], cols: [1, 5], focus: 0, weights: {} }))).toEqual([['a1'], ['a2']]);
    const l = migrateLayout({ panes: ['a1', 'a1', 7, 'a2'], focus: 9, weights: { a1: 'x', a2: 3 } });
    expect(l.panes).toEqual(['a1', 'a2']);
    expect(l.focus).toBe(1);
    expect(l.weights).toEqual({ a2: 3 });
  });

  it('anything that is not a layout becomes the empty layout', () => {
    expect(migrateLayout(null)).toEqual(emptyLayout());
    expect(migrateLayout('x')).toEqual(emptyLayout());
    expect(migrateLayout({ focus: 1 })).toEqual(emptyLayout());
  });
});

describe('stack resize', () => {
  it('resizeStack gives the dragged height to the upper pane and keeps the pair total', () => {
    const l = resizeStack(stacked([['a1', 'a2']], 0), 1, 300, 1000);
    expect(l.heights?.a1).toBeCloseTo(0.6);
    expect(l.heights?.a2).toBeCloseTo(1.4);
    expect(topShare(l, 1)).toBeCloseTo(0.3);
  });

  it(`keeps both panes at least ${MIN_PANE_HEIGHT_PX}px high`, () => {
    const l = stacked([['a1', 'a2']]);
    expect(topShare(resizeStack(l, 1, 5, 1000), 1)).toBeCloseTo(MIN_PANE_HEIGHT_PX / 1000);
    expect(topShare(resizeStack(l, 1, 995, 1000), 1)).toBeCloseTo((1000 - MIN_PANE_HEIGHT_PX) / 1000);
  });

  it('ignores a divider that is not inside a column, and a pair too short for two minimum panes', () => {
    const flat = layout(['a1', 'a2']);
    expect(resizeStack(flat, 1, 300, 1000)).toBe(flat);
    const l = stacked([['a1', 'a2']]);
    expect(resizeStack(l, 1, 100, 2 * MIN_PANE_HEIGHT_PX)).toBe(l);
    expect(resizeStack(l, 0, 300, 1000)).toBe(l);
  });

  it('only touches the two panes around the divider and leaves column widths alone', () => {
    const l = resizeStack(stacked([['a1', 'a2', 'a3']], 0, { weights: { a1: 3 }, heights: { a3: 3 } }), 1, 500, 1000);
    expect(l.heights?.a3).toBe(3);
    expect(weightOf(l, 'a1')).toBe(3);
  });

  it('resizePair drags the divider between columns, using the weight of each column', () => {
    const l = resizePair(stacked([['a1', 'a2'], ['a3']], 0), 2, 300, 1000);
    expect(leftShare(l, 2)).toBeCloseTo(0.3);
    expect(weightOf(l, 'a1')).toBeCloseTo(0.6);
    expect(weightOf(l, 'a3')).toBeCloseTo(1.4);
    // index 1 is a divider inside the column, not between columns
    const same = stacked([['a1', 'a2'], ['a3']]);
    expect(resizePair(same, 1, 300, 1000)).toBe(same);
  });
});

describe('dropDirection', () => {
  it('the right half of a pane splits right, the bottom half splits down', () => {
    expect(dropDirection(900, 100, 1000, 800)).toBe('right');
    expect(dropDirection(100, 700, 1000, 800)).toBe('down');
  });

  it('near the right edge splits right, near the bottom edge splits down', () => {
    expect(dropDirection(980, 600, 1000, 800)).toBe('right');
    expect(dropDirection(600, 790, 1000, 800)).toBe('down');
  });

  it('the top-left quarter and a pane with no size keep the old behaviour: right', () => {
    expect(dropDirection(100, 100, 1000, 800)).toBe('right');
    expect(dropDirection(0, 0, 0, 0)).toBe('right');
  });
});
