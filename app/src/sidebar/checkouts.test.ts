import { describe, expect, it } from 'vitest';
import type { Session } from '../lib/client/types';
import { groupByCheckout } from './checkouts';

const s = (id: string, git: Partial<Session['git']>, parent?: string): Session =>
  ({ id, parent_session: parent, git: { is_git: true, branch: 'main', dirty: 0, ...git } }) as unknown as Session;

describe('groupByCheckout', () => {
  it('puts sessions of the main checkout in one group, with its branch and changes', () => {
    const g = groupByCheckout([s('a', { dirty: 18, added: 284, removed: 104 }), s('b', {})], () => false);
    expect(g).toHaveLength(1);
    expect(g[0]).toMatchObject({ main: true, branch: 'main', summary: { dirty: 18, added: 284, removed: 104 } });
    expect(g[0]?.sessions.map((x) => x.id)).toEqual(['a', 'b']);
  });

  it('gives each worktree its own group after the main one, whatever the order of the sessions', () => {
    const g = groupByCheckout(
      [s('w1', { worktree: true, worktree_path: '/w/auth', branch: 'feature/auth' }), s('m', {}), s('w2', { worktree: true, worktree_path: '/w/tabs', branch: 'fix/tabs' }), s('w3', { worktree: true, worktree_path: '/w/auth', branch: 'feature/auth' })],
      () => false,
    );
    expect(g.map((x) => [x.main, x.branch, x.sessions.map((y) => y.id)])).toEqual([
      [true, 'main', ['m']],
      [false, 'feature/auth', ['w1', 'w3']],
      [false, 'fix/tabs', ['w2']],
    ]);
  });

  it('keeps a child in its parent\'s group, even when it reports a different checkout', () => {
    const g = groupByCheckout([s('p', { worktree: true, worktree_path: '/w/auth', branch: 'feature/auth' }), s('kid', {}, 'p')], (x) => x.id === 'kid');
    expect(g).toHaveLength(1);
    expect(g[0]?.sessions.map((x) => x.id)).toEqual(['p', 'kid']);
  });

  it('treats a session without git as part of the main checkout, with no summary', () => {
    const g = groupByCheckout([s('t', { is_git: false })], () => false);
    expect(g[0]).toMatchObject({ main: true, summary: null });
  });
});
