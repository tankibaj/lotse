import { describe, expect, it } from 'vitest';
import {
  addEditor,
  addPane,
  collapseColumn,
  columnMode,
  focusPane,
  hideColumn,
  keepPanes,
  layoutKey,
  limitPanes,
  paneColumn,
  paneKey,
} from './layout';
import type { PaneLayout } from './layout';

const row = (panes: PaneLayout['panes'], focus = 0): PaneLayout => ({ panes, focus, weights: {} });
const editor = (sessionId: string, path: string) => ({ kind: 'editor' as const, sessionId, path });

describe('focusing reopens the column (review 1)', () => {
  it('focusPane brings a railed or hidden column back', () => {
    const railed = collapseColumn(row(['a1', 'a2', 'a3']), 1);
    const l = focusPane(railed, 1);
    expect(l.focus).toBe(1);
    expect(columnMode(l, 1)).toBe('open');
    const hidden = hideColumn(row(['a1', 'a2', 'a3']), 2);
    expect(columnMode(focusPane(hidden, 2), 2)).toBe('open');
  });

  it('addPane on an open session reopens its hidden column', () => {
    const l = hideColumn(row(['a1', 'a2']), 1);
    const { layout, outcome } = addPane(l, 'a2');
    expect(outcome).toBe('focused');
    expect(layout.focus).toBe(1);
    expect(columnMode(layout, 1)).toBe('open');
  });

  it('addEditor on an open editor reopens its hidden column', () => {
    const ed = editor('a1', 'x.ts');
    const l = hideColumn(row(['a1', ed]), 1);
    const { layout, outcome } = addEditor(l, 'a1', 'x.ts');
    expect(outcome).toBe('focused');
    expect(layout.focus).toBe(1);
    expect(columnMode(layout, 1)).toBe('open');
  });
});

describe('unsaved editors survive a restore (review 2, 5)', () => {
  const ed = editor('a1', 'x.ts');

  it('keepPanes appends the kept editors to a restored layout without moving its focus', () => {
    const merged = keepPanes(row(['a3', 'a2'], 1), row(['a1', ed]), new Set([paneKey(ed)]));
    expect(merged.panes).toEqual(['a3', 'a2', ed]);
    expect(merged.focus).toBe(1);
  });

  it('keepPanes ignores editors that are not kept or already restored', () => {
    expect(keepPanes(row(['a3']), row(['a1', ed]), new Set()).panes).toEqual(['a3']);
    expect(keepPanes(row(['a3', ed]), row(['a1', ed]), new Set([paneKey(ed)])).panes).toEqual(['a3', ed]);
  });

  it('limitPanes never closes a kept pane', () => {
    const l = limitPanes(row(['a1', 'a2', 'a3', 'a4', ed, 'a5']), 4, new Set([paneKey(ed)]));
    expect(l.panes).toContainEqual(ed);
    expect(l.panes.filter((p) => typeof p === 'string')).toEqual(['a1', 'a2', 'a3']);
  });
});

describe('paneColumn', () => {
  it('lists the panes of the column holding a flat index', () => {
    const l: PaneLayout = { panes: ['a1', 'a2', 'a3'], cols: [1, 2], focus: 0, weights: {} };
    expect(paneColumn(l, 2)).toEqual(['a2', 'a3']);
    expect(paneColumn(l, 0)).toEqual(['a1']);
  });
});

describe('layout key (review 10)', () => {
  const base = { name: 'Studio Display', width: 2560, height: 1440 };

  it('tells the same monitor at another scale apart', () => {
    expect(layoutKey({ ...base, scale: 2 })).not.toBe(layoutKey({ ...base, scale: 1 }));
    expect(layoutKey({ ...base, scale: 2 })).toBe(layoutKey({ ...base, scale: 2 }));
  });

  it('uses the position when the monitor has no name', () => {
    const nameless = { name: 'display', width: 1920, height: 1080, scale: 1 };
    expect(layoutKey({ ...nameless, x: 0, y: 0 })).not.toBe(layoutKey({ ...nameless, x: 1920, y: 0 }));
  });

  it('ignores the position of a named monitor, so rearranging displays keeps the layout', () => {
    expect(layoutKey({ ...base, x: 0, y: 0 })).toBe(layoutKey({ ...base, x: 1440, y: -200 }));
  });
});
