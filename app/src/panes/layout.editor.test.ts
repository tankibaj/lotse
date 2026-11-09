import { describe, expect, it } from 'vitest';
import { addEditor, addPane, closePane, columnsOf, focusStep, isEditor, paneKey, sessionOf, syncLayout, weightOf } from './layout';
import type { EditorPane, PaneLayout } from './layout';

const ed = (sessionId: string, path: string): EditorPane => ({ kind: 'editor', sessionId, path });
const layout = (panes: PaneLayout['panes'], focus = 0, extra: Partial<PaneLayout> = {}): PaneLayout => ({ panes, focus, weights: {}, ...extra });
const all = () => true;

describe('pane refs', () => {
  it('a session pane is its id, an editor pane is keyed by session and path', () => {
    expect(paneKey('a1')).toBe('a1');
    expect(paneKey(ed('a1', 'src/x.ts'))).not.toBe(paneKey(ed('a2', 'src/x.ts')));
    expect(paneKey(ed('a1', 'src/x.ts'))).not.toBe(paneKey(ed('a1', 'src/y.ts')));
    expect(isEditor(ed('a1', 'x'))).toBe(true);
    expect(isEditor('a1')).toBe(false);
    expect(sessionOf(ed('a1', 'x'))).toBe('a1');
    expect(sessionOf('a1')).toBe('a1');
  });
});

describe('addEditor', () => {
  it('opens the editor in a column right of the session terminal and focuses it', () => {
    const { layout: l, outcome } = addEditor(layout(['a1', 'a2'], 1), 'a1', 'src/x.ts');
    expect(outcome).toBe('added');
    expect(l.panes).toEqual(['a1', ed('a1', 'src/x.ts'), 'a2']);
    expect(l.focus).toBe(1);
    expect(columnsOf(l)).toHaveLength(3);
  });

  it('a second file of the focused editor opens right of that editor', () => {
    const one = addEditor(layout(['a1', 'a2'], 0), 'a1', 'a.ts').layout;
    const { layout: l } = addEditor(one, 'a1', 'b.ts');
    expect(l.panes).toEqual(['a1', ed('a1', 'a.ts'), ed('a1', 'b.ts'), 'a2']);
    expect(l.focus).toBe(2);
  });

  it('opening a file that is already open focuses it instead of duplicating it', () => {
    const open = layout(['a1', ed('a1', 'a.ts'), 'a2'], 2);
    const { layout: l, outcome } = addEditor(open, 'a1', 'a.ts');
    expect(outcome).toBe('focused');
    expect(l.panes).toEqual(open.panes);
    expect(l.focus).toBe(1);
  });

  it('the same path in another session is another editor', () => {
    const { layout: l, outcome } = addEditor(layout(['a1', ed('a1', 'a.ts')], 1), 'a2', 'a.ts');
    expect(outcome).toBe('added');
    expect(l.panes).toHaveLength(3);
  });

  it('when full it replaces the last editor, never a terminal', () => {
    const full = layout(['a1', ed('a1', 'a.ts'), 'a2', ed('a2', 'b.ts')], 0);
    const { layout: l, outcome } = addEditor(full, 'a1', 'c.ts');
    expect(outcome).toBe('replaced');
    expect(l.panes).toEqual(['a1', ed('a1', 'a.ts'), 'a2', ed('a1', 'c.ts')]);
    expect(l.focus).toBe(3);
  });

  it('when full of terminals it changes nothing', () => {
    const full = layout(['a1', 'a2', 'a3', 'a4'], 0);
    const { layout: l, outcome } = addEditor(full, 'a1', 'c.ts');
    expect(outcome).toBe('full');
    expect(l).toBe(full);
  });

  it('the replaced editor hands its column width to the new one', () => {
    const full = layout(['a1', ed('a1', 'a.ts'), 'a2', 'a3'], 0, { weights: { a1: 1, [paneKey(ed('a1', 'a.ts'))]: 2.5, a2: 1, a3: 1 } });
    const { layout: l } = addEditor(full, 'a1', 'c.ts');
    expect(weightOf(l, paneKey(ed('a1', 'c.ts')))).toBe(2.5);
    expect(weightOf(l, paneKey(ed('a1', 'a.ts')))).toBe(1);
  });

  it('a session whose terminal is not open anchors at the focused pane', () => {
    const { layout: l } = addEditor(layout(['a1', 'a2'], 1), 'a9', 'x.ts');
    expect(l.panes).toEqual(['a1', 'a2', ed('a9', 'x.ts')]);
  });
});

describe('with editors open', () => {
  const l = layout(['a1', ed('a1', 'a.ts'), 'a2'], 1);

  it('addPane counts editors toward the pane limit and keeps their order', () => {
    const { layout: next } = addPane(l, 'a3');
    expect(next.panes).toEqual(['a1', ed('a1', 'a.ts'), 'a3', 'a2']);
    expect(addPane(layout(['a1', ed('a1', 'a.ts'), 'a2', 'a3'], 0), 'a4').outcome).toBe('full');
  });

  it('⌘[ ⌘] step through editors too', () => {
    expect(focusStep(l, 1).focus).toBe(2);
    expect(focusStep(l, -1).focus).toBe(0);
  });

  it('closes an editor like any pane, focus moving left', () => {
    const next = closePane(l, 1);
    expect(next.panes).toEqual(['a1', 'a2']);
    expect(next.focus).toBe(0);
  });

  it('never closes the last terminal while an editor is open', () => {
    const only = layout(['a1', ed('a1', 'a.ts')], 0);
    expect(closePane(only, 0)).toBe(only);
    expect(closePane(only, 1).panes).toEqual(['a1']);
  });
});

describe('syncLayout with editors', () => {
  it('selecting the session of the focused editor keeps focus on the editor', () => {
    const l = layout(['a1', ed('a1', 'a.ts')], 1);
    expect(syncLayout(l, 'a1', all)).toBe(l);
  });

  it('selecting another open session focuses its terminal', () => {
    const l = layout(['a1', ed('a1', 'a.ts'), 'a2'], 1);
    expect(syncLayout(l, 'a2', all).focus).toBe(2);
  });

  it('a new selection replaces the nearest terminal when an editor has focus, not the editor', () => {
    const l = layout(['a1', ed('a1', 'a.ts'), 'a2'], 1);
    const next = syncLayout(l, 'a4', all);
    expect(next.panes).toEqual(['a4', ed('a1', 'a.ts'), 'a2']);
    expect(next.focus).toBe(0);
  });

  it('editors of a session that is gone close with it', () => {
    const l = layout(['a1', ed('a2', 'a.ts'), 'a2'], 0);
    expect(syncLayout(l, 'a1', (id) => id !== 'a2').panes).toEqual(['a1']);
  });
});
