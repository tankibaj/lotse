import { describe, expect, it } from 'vitest';
import {
  MAX_PANES,
  addEditor,
  addPane,
  closePane,
  collapseColumn,
  columnMode,
  focusStep,
  hideColumn,
  layoutKey,
  limitPanes,
  loadLayout,
  maxPanesFor,
  restoreColumn,
  saveLayout,
  syncLayout,
} from './layout';
import type { PaneLayout } from './layout';

const row = (panes: string[], focus = 0, extra: Partial<PaneLayout> = {}): PaneLayout => ({ panes, focus, weights: {}, ...extra });
const all = () => true;
const memory = () => {
  const data = new Map<string, string>();
  return { getItem: (k: string) => data.get(k) ?? null, setItem: (k: string, v: string) => void data.set(k, v) };
};

describe('rail and hide', () => {
  it('collapse_to_rail_keeps_session', () => {
    const l = collapseColumn(row(['a1', 'a2', 'a3'], 1), 1);
    expect(l.panes).toEqual(['a1', 'a2', 'a3']);
    expect(columnMode(l, 1)).toBe('rail');
    expect(columnMode(l, 0)).toBe('open');
    expect(l.focus).not.toBe(1); // focus leaves the rail for the nearest open column
    expect(columnMode(l, l.focus)).toBe('open');
  });

  it('hidden_column_restores', () => {
    const hidden = hideColumn(row(['a1', 'a2', 'a3'], 1), 1);
    expect(columnMode(hidden, 1)).toBe('hidden');
    expect(hidden.panes).toEqual(['a1', 'a2', 'a3']);
    // selecting the session (sidebar card, tab) brings its column back and focuses it
    const synced = syncLayout(hidden, 'a2', all);
    expect(columnMode(synced, 1)).toBe('open');
    expect(synced.focus).toBe(1);
    // so does the explicit restore, from the rail
    const railed = collapseColumn(row(['a1', 'a2'], 1), 1);
    const back = restoreColumn(railed, 1);
    expect(columnMode(back, 1)).toBe('open');
    expect(back.focus).toBe(1);
  });

  it('keeps one column open', () => {
    const one = collapseColumn(row(['a1', 'a2'], 0), 0);
    expect(columnMode(one, 0)).toBe('rail');
    expect(hideColumn(one, 1)).toBe(one);
    expect(collapseColumn(row(['a1']), 0).collapsed).toBeUndefined();
  });

  it('⌘[ ⌘] skip railed and hidden columns', () => {
    const l = hideColumn(collapseColumn(row(['a1', 'a2', 'a3', 'a4'], 0), 1), 2);
    expect(focusStep(l, 1).focus).toBe(3);
    expect(focusStep(focusStep(l, 1), 1).focus).toBe(0);
  });

  it('state follows the column when panes are added or closed', () => {
    const l = collapseColumn(row(['a1', 'a2', 'a3'], 0), 1);
    const added = addPane(l, 'a4').layout; // right of the focused a1
    expect(added.panes).toEqual(['a1', 'a4', 'a2', 'a3']);
    expect(columnMode(added, 2)).toBe('rail');
    expect(columnMode(closePane(added, 0), 1)).toBe('rail'); // a2
  });

  it('closing the focused pane next to a railed column never leaves focus on the rail', () => {
    const l = collapseColumn(row(['a1', 'a2'], 1), 1); // focus a1, a2 railed
    const closed = closePane(l, 0);
    expect(closed.panes).toEqual(['a2']);
    expect(columnMode(closed, 0)).toBe('open');
  });

  it('drops state of columns that are gone', () => {
    const l = hideColumn(row(['a1', 'a2'], 0), 1);
    const synced = syncLayout(l, 'a1', (id) => id !== 'a2');
    expect(synced.panes).toEqual(['a1']);
    expect(synced.collapsed).toBeUndefined();
  });
});

describe('displays', () => {
  it('layout_key_per_display', () => {
    const macbook = { name: 'Built-in Retina Display', width: 1512, height: 982 };
    const studio = { name: 'Studio Display', width: 3440, height: 1440 };
    expect(layoutKey(macbook)).toBe(layoutKey({ ...macbook }));
    expect(layoutKey(macbook)).not.toBe(layoutKey(studio));
    expect(layoutKey({ ...studio, width: 2560 })).not.toBe(layoutKey(studio)); // same name, other resolution
  });

  it('max_panes_by_width', () => {
    expect(maxPanesFor(1440)).toBe(MAX_PANES);
    expect(maxPanesFor(2999)).toBe(4);
    expect(maxPanesFor(3000)).toBe(6);
    expect(maxPanesFor(5120)).toBe(6);
    let l = row(['a1']);
    for (const id of ['a2', 'a3', 'a4', 'a5', 'a6']) l = addPane(l, id, 'right', 6).layout;
    expect(l.panes).toHaveLength(6);
    expect(addPane(l, 'a7', 'right', 6).outcome).toBe('full');
    expect(addPane(row(['a1', 'a2', 'a3', 'a4']), 'a5').outcome).toBe('full');
    expect(addEditor(l, 'a1', 'x.ts', 6).outcome).toBe('full');
  });

  it('limit_panes_to_max trims the tail but keeps the focus', () => {
    const l = limitPanes(row(['a1', 'a2', 'a3', 'a4', 'a5', 'a6'], 1), 4);
    expect(l.panes).toEqual(['a1', 'a2', 'a3', 'a4']);
    expect(l.focus).toBe(1);
  });

  it('saves and loads a layout per key, with rail state, without editors', () => {
    const storage = memory();
    const l: PaneLayout = collapseColumn(row(['a1', 'a2', 'a3'], 0, { weights: { a1: 2 } }), 2);
    const withEditor = addEditor(l, 'a1', 'src/x.ts').layout;
    saveLayout(storage, 'k1', withEditor);
    const back = loadLayout(storage, 'k1');
    expect(back?.panes).toEqual(['a1', 'a2', 'a3']);
    expect(back?.weights).toEqual({ a1: 2 });
    expect(back && columnMode(back, 2)).toBe('rail');
    expect(loadLayout(storage, 'k2')).toBeNull();
    storage.setItem('bad', '{nope');
    expect(loadLayout(storage, 'bad')).toBeNull();
  });
});
