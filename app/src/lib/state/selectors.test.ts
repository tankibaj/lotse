import { describe, expect, it } from 'vitest';
import type { Project, Session, Status } from '../client/types';
import {
  coarseAge,
  displayStatus,
  filterGroups,
  formatTokens,
  formatUsage,
  groupSessions,
  groupWorkspaces,
  mostUrgent,
  needsYou,
  noLiveStatus,
  NO_SIGNAL_MS,
  needYouCount,
  nextNeedYou,
  rollup,
  rollupSummary,
  sessionAge,
  shortBranch,
  STATUS_LABEL,
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

const sub = (id: string, status: Status) => ({ id, name: id, status });

describe('running-lead rule', () => {
  it('a lead below run shows run while any sub-agent runs; perm and fail stay', () => {
    const running = [sub('x', 'done'), sub('y', 'run')];
    for (const st of ['idle', 'unknown', 'done'] as const) {
      expect(displayStatus(session('a', st, { sub_agents: running }))).toBe('run');
    }
    expect(displayStatus(session('a', 'perm', { sub_agents: running }))).toBe('perm');
    expect(displayStatus(session('a', 'fail', { sub_agents: running }))).toBe('fail');
    expect(displayStatus(session('a', 'idle', { sub_agents: [sub('x', 'done')] }))).toBe('idle');
    expect(displayStatus(session('a', 'unknown'))).toBe('unknown');
  });

  it('the collapsed roll-up uses it, so an idle lead with a running sub-agent rolls up as run', () => {
    expect(rollup([session('a', 'idle', { sub_agents: [sub('y', 'run')] })])).toBe('run');
    expect(rollupSummary([session('a', 'unknown', { sub_agents: [sub('y', 'run')] })]).top).toBe('run');
  });
});

describe('status labels', () => {
  it('are plain words and distinct; unknown reads as detecting', () => {
    expect(STATUS_LABEL).toEqual({
      run: 'Running',
      perm: 'Needs permission',
      fail: 'Failed',
      done: 'Done',
      idle: 'Idle',
      unknown: 'Detecting…',
    });
  });
});

describe('no live status hint', () => {
  const s = (over: Partial<Session> = {}) => session('a', 'unknown', { created_at: 1000, source: 'process', ...over });
  it('appears for an unknown agent session after 10 s without a signal, never for terminals or known states', () => {
    expect(noLiveStatus(s(), 1000 + NO_SIGNAL_MS - 1)).toBe(false);
    expect(noLiveStatus(s(), 1000 + NO_SIGNAL_MS)).toBe(true);
    for (const agent of ['claude_code', 'codex', 'opencode'] as const) {
      expect(noLiveStatus(s({ agent }), 20_000)).toBe(true);
    }
    expect(noLiveStatus(s({ agent: 'terminal' }), 20_000)).toBe(false);
    expect(noLiveStatus(session('a', 'idle', { created_at: 0 }), 20_000)).toBe(false);
  });

  it('flips the time signature once, so the sidebar re-renders once and not per second', () => {
    const list = [s()];
    expect(timeSignature(list, 5_000)).toBe(timeSignature(list, 9_000));
    expect(timeSignature(list, 9_000)).not.toBe(timeSignature(list, 12_000));
    expect(timeSignature(list, 12_000)).toBe(timeSignature(list, 15_000));
  });
});

describe('workspaces', () => {
  const project = (id: string, workspace_id: string): Project => ({
    id,
    workspace_id,
    name: id,
    path: `/x/${id}`,
    color: 'teal',
    icon: 'folder',
    is_git: true,
    notifications: { mode: 'global' },
  });
  const ws = [
    { id: 'w1', name: 'Default' },
    { id: 'w2', name: 'Client work' },
    { id: 'w3', name: 'Empty' },
  ];

  it('groups projects under their workspace in daemon order, with the workspace\'s sessions for the roll-up', () => {
    const projects = [project('p1', 'w1'), project('p2', 'w2'), project('p3', 'w1')];
    const sessions = [session('a', 'idle', { project_id: 'p1' }), session('b', 'perm', { project_id: 'p2' }), session('c', 'run', { project_id: 'p3' })];
    const groups = groupWorkspaces(ws, groupSessions(projects, sessions));
    expect(groups.map((g) => [g.workspace.id, g.groups.map((x) => x.project.id)])).toEqual([
      ['w1', ['p1', 'p3']],
      ['w2', ['p2']],
      ['w3', []],
    ]);
    // the most urgent state rolls up to the workspace: perm > run > idle
    expect(groups.map((g) => rollup(g.sessions))).toEqual(['run', 'perm', 'idle']);
    expect(rollupSummary(groups[1]!.sessions)).toMatchObject({ perm: 1, fail: 0, top: 'perm' });
  });

  it('a project of an unknown workspace sits in the first one instead of vanishing', () => {
    const groups = groupWorkspaces(ws, groupSessions([project('p1', 'gone')], []));
    expect(groups[0]?.groups.map((g) => g.project.id)).toEqual(['p1']);
    expect(groups[1]?.groups).toEqual([]);
  });
});

describe('today\'s usage text', () => {
  const usage = (input: number, cost: number | null, rest = 0) => ({
    input_tokens: input,
    output_tokens: rest,
    cache_read_tokens: 0,
    cache_write_tokens: 0,
    cost_micros: cost,
  });

  it('formats token counts compactly', () => {
    expect([0, 999, 1_000, 1_234, 9_960, 10_000, 412_000, 999_499, 1_000_000, 1_500_000, 12_345_678].map(formatTokens)).toEqual([
      '0',
      '999',
      '1k',
      '1.2k',
      '10k', // 9_960 rounds up
      '10k',
      '412k',
      '999k',
      '1M',
      '1.5M',
      '12.3M',
    ]);
  });

  it('rounds first, then picks the unit: no 1000k, no 10.0k', () => {
    expect([9_949, 9_950, 9_999, 10_499, 10_500, 999_499, 999_500, 999_999].map(formatTokens)).toEqual([
      '9.9k',
      '10k',
      '10k',
      '10k',
      '11k',
      '999k',
      '1M',
      '1M',
    ]);
  });

  it('reads like the mock: Today 412k tokens · $4.18', () => {
    expect(formatUsage(usage(412_000, 4_180_000)).text).toBe('Today 412k tokens · $4.18');
    expect(formatUsage(usage(0, 0)).text).toBe('Today 0 tokens · $0.00');
    expect(formatUsage(usage(1_000, 6_000)).text).toBe('Today 1k tokens · $0.01'); // cents round to nearest
  });

  it('is tokens only without a price, dashes before the first snapshot', () => {
    const noPrice = formatUsage(usage(2_000_000, null, 500_000));
    expect(noPrice.text).toBe('Today 2.5M tokens');
    expect(noPrice.title).toContain('without a known price');
    expect(formatUsage(null).text).toBe('Today — tokens · —');
  });
});
