import { act, renderHook, waitFor } from '@testing-library/react';
import type { ReactNode } from 'react';
import { describe, expect, it, vi } from 'vitest';
import { ClientProvider, MockClient } from '../client';
import type { DaemonEvent } from '../client/types';
import { getSelectedSessionId, selectSession } from '../ui/selection';
import { getStore, INITIAL_STATE, reduceEvent, resetUiState, subscribeNeedYouCount, useWorkspace } from './index';
import type { WorkspaceState } from './index';

const wrapper = (client: MockClient) =>
  function W({ children }: { children: ReactNode }) {
    return <ClientProvider client={client}>{children}</ClientProvider>;
  };

async function load(client = new MockClient({ script: false })) {
  selectSession(null);
  resetUiState();
  const hook = renderHook(() => useWorkspace(), { wrapper: wrapper(client) });
  await waitFor(() => expect(hook.result.current.loaded).toBe(true));
  return { client, ...hook };
}

/** Snapshot calls can be held (gate) or failed, to exercise the load / event race. */
class GatedClient extends MockClient {
  holdFirst = false;
  failFirst = false;
  releases: (() => void)[] = [];
  private calls = 0;
  override async getSnapshot() {
    const call = this.calls++;
    if (this.failFirst && call === 0) throw new Error('daemon down');
    const snap = await super.getSnapshot(); // taken now: a later event makes it stale
    if (this.holdFirst && call === 0) await new Promise<void>((r) => this.releases.push(r));
    return snap;
  }
}

const perm = { tool: 'Bash', summary: 'ls' };
const statusOf = (r: { current: WorkspaceState }, id: string) => r.current.sessions.find((s) => s.id === id)?.status;

describe('snapshot / event race', () => {
  const start = async (client: GatedClient) => {
    selectSession(null);
    resetUiState();
    const hook = renderHook(() => useWorkspace(), { wrapper: wrapper(client) });
    return hook;
  };

  it('replays events that arrive while the snapshot is in flight on top of it', async () => {
    const client = new GatedClient({ script: false });
    client.holdFirst = true;
    const { result } = await start(client);
    await waitFor(() => expect(client.releases).toHaveLength(1));
    act(() => client.setStatus('a2', 'perm', perm)); // not in the (already taken) snapshot
    await act(async () => client.releases[0]?.());
    await waitFor(() => expect(result.current.loaded).toBe(true));
    expect(statusOf(result, 'a2')).toBe('perm');
  });

  it('a newer load wins over an older one that resolves late', async () => {
    const client = new GatedClient({ script: false });
    client.holdFirst = true;
    const { result } = await start(client);
    await waitFor(() => expect(client.releases).toHaveLength(1));
    act(() => client.setStatus('a2', 'perm', perm));
    act(() => client.setConnection('reconnected')); // second load: fresh snapshot, resolves at once
    await waitFor(() => expect(statusOf(result, 'a2')).toBe('perm'));
    await act(async () => client.releases[0]?.()); // the stale one (a2 idle) arrives late
    expect(statusOf(result, 'a2')).toBe('perm');
  });

  it('drops a snapshot that resolves after the last subscriber left', async () => {
    const client = new GatedClient({ script: false });
    client.holdFirst = true;
    const { unmount } = await start(client);
    await waitFor(() => expect(client.releases).toHaveLength(1));
    unmount();
    await act(async () => client.releases[0]?.());
    expect(getStore(client).getState().loaded).toBe(false);
  });

  it('a failed snapshot shows the daemon as down and the next reconnect recovers', async () => {
    const client = new GatedClient({ script: false });
    client.failFirst = true;
    const { result } = await start(client);
    await waitFor(() => expect(result.current.connection).toBe('disconnected'));
    expect(result.current.loaded).toBe(false);
    act(() => client.setConnection('reconnected'));
    await waitFor(() => expect(result.current.loaded).toBe(true));
    expect(result.current.connection).toBe('connected');
    act(() => client.setStatus('a2', 'run')); // events apply live again
    expect(statusOf(result, 'a2')).toBe('run');
  });
});

describe('reduceEvent', () => {
  const base = async (): Promise<WorkspaceState> => {
    const snap = await new MockClient({ script: false }).getSnapshot();
    return { ...INITIAL_STATE, loaded: true, daemon: snap.daemon, projects: snap.projects, sessions: snap.sessions, settings: snap.settings };
  };
  const apply = async (e: DaemonEvent) => reduceEvent(await base(), e);

  it('patches status, perm and activity in place and leaves other sessions untouched', async () => {
    const before = await base();
    const after = reduceEvent(before, {
      event: 'status_changed',
      data: { session_id: 'a2', status: 'perm', source: 'hook', perm: { tool: 'Bash', summary: 'ls' }, running_since: null, at: 5 },
    });
    expect(after.sessions.find((s) => s.id === 'a2')).toMatchObject({ status: 'perm', perm: { tool: 'Bash' }, last_activity: 5 });
    expect(after.sessions.find((s) => s.id === 'a1')).toBe(before.sessions.find((s) => s.id === 'a1'));
  });

  it('adds, updates and closes sessions, and drops a removed project with its sessions', async () => {
    const s = (await base()).sessions[0];
    if (!s) throw new Error('fixture has no session');
    expect((await apply({ event: 'session_created', data: { ...s, id: 'new' } })).sessions).toHaveLength(8);
    expect((await apply({ event: 'session_created', data: s })).sessions).toHaveLength(7); // duplicate ignored
    expect((await apply({ event: 'session_closed', data: { session_id: 'a1' } })).sessions.some((x) => x.id === 'a1')).toBe(false);
    const removed = await apply({ event: 'project_removed', data: { project_id: 'p1' } });
    expect(removed.projects).toHaveLength(4);
    expect(removed.sessions.some((x) => x.project_id === 'p1')).toBe(false);
  });

  it('applies sub-agent, git and settings events', async () => {
    const sub = await apply({ event: 'sub_agent_status', data: { session_id: 'a1', sub_agent_id: 'a1-s2', status: 'done' } });
    expect(sub.sessions.find((s) => s.id === 'a1')?.sub_agents.find((x) => x.id === 'a1-s2')?.status).toBe('done');
    const git = await apply({
      event: 'git_info_changed',
      data: { session_id: 'a1', git: { is_git: true, branch: 'x', ahead: 9, dirty: 0, worktree: false, worktree_path: null } },
    });
    expect(git.sessions.find((s) => s.id === 'a1')?.git.ahead).toBe(9);
  });

  it('sub_agents_changed replaces the list (start and stop); sub_agent_status updates one row', async () => {
    const started = await apply({
      event: 'sub_agents_changed',
      data: { session_id: 'a2', sub_agents: [{ id: 'a2-s1', name: 'schema-check', status: 'run' }] },
    });
    expect(started.sessions.find((s) => s.id === 'a2')?.sub_agents).toEqual([{ id: 'a2-s1', name: 'schema-check', status: 'run' }]);
    const stopped = reduceEvent(started, { event: 'sub_agent_status', data: { session_id: 'a2', sub_agent_id: 'a2-s1', status: 'done' } });
    expect(stopped.sessions.find((s) => s.id === 'a2')?.sub_agents.map((x) => x.status)).toEqual(['done']);
    const cleared = reduceEvent(stopped, { event: 'sub_agents_changed', data: { session_id: 'a2', sub_agents: [] } });
    expect(cleared.sessions.find((s) => s.id === 'a2')?.sub_agents).toEqual([]);
    // a status for a sub-agent that was never announced adds no row
    const stray = reduceEvent(cleared, { event: 'sub_agent_status', data: { session_id: 'a2', sub_agent_id: 'ghost', status: 'run' } });
    expect(stray.sessions.find((s) => s.id === 'a2')?.sub_agents).toEqual([]);
  });

  it('ignores events for sessions it does not know', async () => {
    const before = await base();
    const after = reduceEvent(before, { event: 'sub_agent_status', data: { session_id: 'zzz', sub_agent_id: 'x', status: 'done' } });
    expect(after).toBe(before);
  });
});

describe('workspace store', () => {
  it('loads the snapshot and selects the first session', async () => {
    const { result } = await load();
    expect(result.current.projects).toHaveLength(5);
    expect(result.current.sessions).toHaveLength(7);
    expect(getSelectedSessionId()).toBe('a1');
  });

  it('follows daemon events', async () => {
    const { result, client } = await load();
    act(() => client.setStatus('a2', 'run'));
    expect(result.current.sessions.find((s) => s.id === 'a2')?.status).toBe('run');
  });

  it('picks a sibling when the selected session closes, and the next project when none is left', async () => {
    const { client } = await load();
    selectSession('a1');
    await act(() => client.closeSession('a1'));
    expect(getSelectedSessionId()).toBe('a2');
    await act(() => client.closeSession('a2'));
    expect(getSelectedSessionId()).toBe('a3');
  });

  it('marks the selected session read', async () => {
    const { client } = await load();
    const spy = vi.spyOn(client, 'markRead');
    act(() => selectSession('a6'));
    expect(spy).toHaveBeenCalledWith('a6');
  });

  it('refetches the snapshot after a reconnect and shows the daemon as down meanwhile', async () => {
    const { result, client } = await load();
    const snapshot = vi.spyOn(client, 'getSnapshot');
    act(() => client.setConnection('disconnected'));
    expect(result.current.connection).toBe('disconnected');
    act(() => client.setConnection('reconnected'));
    await waitFor(() => expect(result.current.connection).toBe('connected'));
    expect(snapshot).toHaveBeenCalledTimes(1);
  });

  it('is the Dock-badge source: reports the need-you count now and when it changes', async () => {
    const client = new MockClient({ script: false });
    const counts: number[] = [];
    const off = subscribeNeedYouCount(client, (n) => counts.push(n));
    await waitFor(() => expect(counts).toEqual([2]));
    await act(() => client.resolvePermission('a3', 'allow'));
    expect(counts).toEqual([2, 1]);
    off();
  });
});
