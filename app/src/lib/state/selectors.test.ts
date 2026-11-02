import { describe, expect, it } from 'vitest';
import type { Session, Status } from '../client/types';
import {
  coarseAge,
  filterGroups,
  groupSessions,
  mostUrgent,
  needsYou,
  needYouCount,
  nextNeedYou,
  rollup,
  rollupSummary,
  sessionAge,
  shortBranch,
  timeSignature,
  visibleSessionIds,
} from './selectors';

const session = (id: string, status: Status, over: Partial<Session> = {}): Session => ({
  id,
  project_id: 'p1',
  title: id,
  agent: 'claude_code',
  command: 'claude',
  status,
  sub_agents: [],
  git: { is_git: true, branch: 'main', ahead: 0, dirty: 0, worktree: false, worktree_path: null },
  created_at: 0,
  last_activity: 0,
  running_since: null,
  unread: false,
  perm: null,
  source: 'hook',
  ...over,
});

const project = (id: string) => ({
  id,
  workspace_id: 'w1',
  name: id,
  path: `/${id}`,
  color: 'purple',
  icon: 'folder',
  is_git: true,
  notifications: { mode: 'global' as const },
});

describe('roll-up', () => {
  it('orders perm > fail > run > done > idle', () => {
    const order: Status[] = ['perm', 'fail', 'run', 'done', 'idle'];
    for (let i = 0; i < order.length; i++) {
      const rest = order.slice(i).map((st, n) => session(`s${n}`, st));
      expect(rollup(rest.reverse())).toBe(order[i]);
    }
  });

  it('is idle for a project without sessions, and unknown does not outrank idle', () => {
    expect(rollup([])).toBe('idle');
    expect(rollup([session('a', 'unknown')])).toBe('idle');
  });

  it('counts a session once for its own state and its sub-agents', () => {
    const s = session('a', 'perm', { sub_agents: [{ id: 'x', name: 'x', status: 'perm' }] });
    const f = session('b', 'run', { sub_agents: [{ id: 'y', name: 'y', status: 'fail' }] });
    expect(rollupSummary([s, f, session('c', 'fail')])).toEqual({ perm: 1, fail: 2, top: 'perm' });
  });
});

describe('need you', () => {
  it('is perm or fail, own or a sub-agent', () => {
    expect(needsYou(session('a', 'perm'))).toBe(true);
    expect(needsYou(session('a', 'fail'))).toBe(true);
    expect(needsYou(session('a', 'run'))).toBe(false);
    expect(needsYou(session('a', 'run', { sub_agents: [{ id: 'x', name: 'x', status: 'perm' }] }))).toBe(true);
    expect(needYouCount([session('a', 'perm'), session('b', 'done'), session('c', 'fail')])).toBe(2);
  });

  it('⌘J cycles through the sessions that need you, wrapping', () => {
    const list = [session('a', 'run'), session('b', 'perm'), session('c', 'done'), session('d', 'fail')];
    expect(nextNeedYou(list, 'a')).toBe('b');
    expect(nextNeedYou(list, 'b')).toBe('d');
    expect(nextNeedYou(list, 'd')).toBe('b');
    expect(nextNeedYou(list, null)).toBe('b');
    expect(nextNeedYou([session('a', 'run')], 'a')).toBeNull();
  });

  it('picks the most urgent session when the filter turns on', () => {
    expect(mostUrgent([session('a', 'fail'), session('b', 'perm')])).toBe('b');
    expect(mostUrgent([session('a', 'done')])).toBeNull();
  });
});

describe('filter and visible order', () => {
  const groups = groupSessions(
    [project('p1'), project('p2'), project('p3')],
    [
      session('a', 'run', { project_id: 'p1' }),
      session('b', 'perm', { project_id: 'p1' }),
      session('c', 'fail', { project_id: 'p2' }),
      session('d', 'done', { project_id: 'p3' }),
    ],
  );

  it('keeps only needing sessions and drops empty projects', () => {
    const shown = filterGroups(groups, true);
    expect(shown.map((g) => [g.project.id, g.sessions.map((s) => s.id)])).toEqual([
      ['p1', ['b']],
      ['p2', ['c']],
    ]);
    expect(filterGroups(groups, false)).toHaveLength(3);
  });

  it('skips collapsed projects, except under the filter', () => {
    const collapsed = new Set(['p1']);
    expect(visibleSessionIds(groups, collapsed, false)).toEqual(['c', 'd']);
    expect(visibleSessionIds(filterGroups(groups, true), collapsed, true)).toEqual(['b', 'c']);
  });
});

describe('coarse time', () => {
  const min = 60_000;
  it('never shows seconds', () => {
    expect(coarseAge(0)).toBe('<1m');
    expect(coarseAge(59_999)).toBe('<1m');
    expect(coarseAge(4 * min)).toBe('4m');
    expect(coarseAge(60 * min)).toBe('1h');
    expect(coarseAge(65 * min)).toBe('1h 5m');
    expect(coarseAge(49 * 60 * min)).toBe('2d');
    expect(coarseAge(-5)).toBe('<1m');
  });

  it('shows run time for running sessions and last activity for the rest', () => {
    const now = 10 * 60 * min;
    expect(sessionAge(session('a', 'run', { running_since: now - 3 * min, last_activity: now }), now)).toBe('3m');
    expect(sessionAge(session('a', 'idle', { last_activity: now - 90 * min }), now)).toBe('1h 30m');
  });

  it('changes its signature only when a visible label changes', () => {
    const list = [session('a', 'run', { running_since: 0 })];
    expect(timeSignature(list, 61_000)).toBe(timeSignature(list, 119_000));
    expect(timeSignature(list, 61_000)).not.toBe(timeSignature(list, 121_000));
  });
});

it('drops the lotse/ branch prefix', () => {
  expect(shortBranch('lotse/fix-rounding')).toBe('fix-rounding');
  expect(shortBranch('feature/x')).toBe('feature/x');
});
