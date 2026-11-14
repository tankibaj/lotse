import { describe, expect, it } from 'vitest';
import type { Session } from '../client/types';
import { moveTab, orderTabs, projectTabs, saveTabOrder } from './tabOrder';

const s = (id: string, project_id = 'p1') => ({ id, project_id }) as Session;
const ids = (list: Session[]) => list.map((x) => x.id);

describe('moveTab', () => {
  it('puts a tab before another, or at the end', () => {
    expect(moveTab(['a', 'b', 'c'], 'c', 'a')).toEqual(['c', 'a', 'b']);
    expect(moveTab(['a', 'b', 'c'], 'a', null)).toEqual(['b', 'c', 'a']);
    expect(moveTab(['a', 'b', 'c'], 'b', 'c')).toEqual(['a', 'b', 'c']);
  });
});

describe('orderTabs', () => {
  it('follows the saved order, new sessions last, gone ones ignored', () => {
    expect(ids(orderTabs([s('a'), s('b'), s('c'), s('d')], ['c', 'x', 'a']))).toEqual(['c', 'a', 'b', 'd']);
    expect(ids(orderTabs([s('a'), s('b')], undefined))).toEqual(['a', 'b']);
  });
});

describe('projectTabs / saveTabOrder', () => {
  it('keeps one order per project in localStorage and ignores a corrupt value', () => {
    const all = [s('a'), s('b'), s('c'), s('z', 'p2')];
    saveTabOrder('p1', ['c', 'a', 'b']);
    expect(ids(projectTabs(all, 'p1'))).toEqual(['c', 'a', 'b']);
    expect(ids(projectTabs(all, 'p2'))).toEqual(['z']);
    expect(projectTabs(all, undefined)).toEqual([]);
    localStorage.setItem('lotse-tab-order', '{nope');
    expect(ids(projectTabs(all, 'p1'))).toEqual(['a', 'b', 'c']);
  });
});
