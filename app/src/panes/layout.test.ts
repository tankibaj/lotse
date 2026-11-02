import { describe, expect, it } from 'vitest';
import {
  MAX_PANES,
  MIN_PANE_PX,
  addPane,
  closePane,
  emptyLayout,
  focusPane,
  focusStep,
  leftShare,
  nextSessionToOpen,
  resizePair,
  syncLayout,
  weightOf,
} from './layout';
import type { PaneLayout } from './layout';

const layout = (panes: string[], focus = 0, weights: Record<string, number> = {}): PaneLayout => ({ panes, focus, weights });
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
});
