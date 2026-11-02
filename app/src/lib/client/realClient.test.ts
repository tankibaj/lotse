import { afterEach, describe, expect, it, vi } from 'vitest';
import type { Transport } from './realClient';
import { RealClient } from './realClient';
import { chooseClient } from './index';
import { DaemonError } from './types';
import type { DaemonEvent, Request, Response, Snapshot } from './types';
import fixture from '../../../../crates/lotse-proto/fixtures/snapshot_mock.json';

/** A fake host: records calls, answers `daemon_request` through `answer`, lets tests push events and PTY bytes. */
function fakeHost(answer: (r: Request) => Response = () => ({ kind: 'ok' })) {
  const listeners = new Map<string, (p: unknown) => void>();
  let onPty: ((d: ArrayBuffer | number[]) => void) | null = null;
  const calls: { command: string; args?: Record<string, unknown> }[] = [];
  const raw: { command: string; body: Uint8Array; headers: Record<string, string> }[] = [];
  let connectFails = 0;
  const transport: Transport = {
    invoke: vi.fn(async (command: string, args?: Record<string, unknown>) => {
      calls.push({ command, args });
      if (command === 'daemon_connect') {
        if (connectFails > 0) {
          connectFails--;
          throw 'lotsed not found';
        }
        return { proto_version: 2, daemon_version: '0.1.0' };
      }
      if (command === 'daemon_request') return answer(args?.request as Request);
      return undefined;
    }) as Transport['invoke'],
    invokeRaw: vi.fn(async (command, body, headers) => {
      raw.push({ command, body, headers });
    }),
    listen: vi.fn(async (event: string, handler: (p: unknown) => void) => {
      listeners.set(event, handler);
      return () => listeners.delete(event);
    }) as Transport['listen'],
    channel: (cb) => {
      onPty = cb;
      return { channel: true };
    },
  };
  return {
    transport,
    calls,
    raw,
    emit: (event: string, payload: unknown) => listeners.get(event)?.(payload),
    pty: (id: string, bytes: number[]) => {
      const idBytes = [...new TextEncoder().encode(id)];
      onPty?.(new Uint8Array([0, idBytes.length, ...idBytes, ...bytes]).buffer);
    },
    failConnect: (n: number) => (connectFails = n),
  };
}

const snapshot = (fixture as unknown as { body: { response: { data: Snapshot } } }).body.response.data;

afterEach(() => vi.useRealTimers());

describe('RealClient', () => {
  it('connects on first use, listening for events before it connects', async () => {
    const h = fakeHost((r) => (r.method === 'get_snapshot' ? { kind: 'snapshot', data: snapshot } : { kind: 'ok' }));
    const client = new RealClient({ transport: h.transport });
    const snap = await client.getSnapshot();
    expect(snap.projects.length).toBeGreaterThan(0);
    const order = h.calls.map((c) => c.command);
    expect(order.indexOf('daemon_connect')).toBeLessThan(order.indexOf('daemon_request'));
    expect(h.transport.listen).toHaveBeenCalledWith('lotse://event', expect.any(Function));
    expect(h.transport.listen).toHaveBeenCalledWith('lotse://connection', expect.any(Function));
    // the PTY channel travels with the connect call
    expect(h.calls.find((c) => c.command === 'daemon_connect')?.args).toEqual({ pty: { channel: true } });
    // connecting twice does not reconnect
    await client.connect();
    expect(h.calls.filter((c) => c.command === 'daemon_connect')).toHaveLength(1);
  });

  it('maps methods to proto requests (snake_case params, null for missing ids)', async () => {
    const seen: Request[] = [];
    const h = fakeHost((r) => {
      seen.push(r);
      return r.method === 'list_sessions' ? { kind: 'sessions', data: [] } : r.method === 'get_git_info' ? { kind: 'git_info', data: snapshot.sessions[0]!.git } : { kind: 'ok' };
    });
    const c = new RealClient({ transport: h.transport });
    await c.listSessions();
    await c.getGitInfo('p1');
    await c.renameSession('s1', 'New title');
    await c.resolvePermission('s1', 'allow');
    await c.resize('s1', 120, 40);
    await c.markRead('s1');
    expect(seen).toEqual([
      { method: 'list_sessions', params: { project_id: null } },
      { method: 'get_git_info', params: { project_id: 'p1', session_id: null } },
      { method: 'rename_session', params: { session_id: 's1', title: 'New title' } },
      { method: 'resolve_permission', params: { session_id: 's1', decision: 'allow' } },
      { method: 'resize', params: { session_id: 's1', cols: 120, rows: 40 } },
      { method: 'mark_read', params: { session_id: 's1' } },
    ]);
  });

  it('maps the proto 3 methods: workspaces, move_project, get_diff, get_transcript', async () => {
    const seen: Request[] = [];
    const ws = { id: 'w2', name: 'Client work' };
    const h = fakeHost((r) => {
      seen.push(r);
      switch (r.method) {
        case 'create_workspace':
        case 'rename_workspace':
          return { kind: 'workspace', data: ws };
        case 'move_project':
          return { kind: 'project', data: snapshot.projects[0]! };
        case 'get_diff':
          return { kind: 'diff', data: { is_git: true, files: [], truncated: false } };
        case 'get_transcript':
          return { kind: 'transcript', data: { available: false, turns: [], truncated: false } };
        default:
          return { kind: 'ok' };
      }
    });
    const c = new RealClient({ transport: h.transport });
    expect(await c.createWorkspace('Client work')).toEqual(ws);
    expect(await c.renameWorkspace('w2', 'Clients')).toEqual(ws);
    await c.deleteWorkspace('w2', true);
    expect((await c.moveProject('p1', 'w2')).id).toBe('p1');
    expect((await c.getDiff('s1')).is_git).toBe(true);
    expect((await c.getTranscript('s1')).available).toBe(false);
    await c.getTranscript('s1', 20);
    expect(seen).toEqual([
      { method: 'create_workspace', params: { name: 'Client work' } },
      { method: 'rename_workspace', params: { workspace_id: 'w2', name: 'Clients' } },
      { method: 'delete_workspace', params: { workspace_id: 'w2', move_projects: true } },
      { method: 'move_project', params: { project_id: 'p1', workspace_id: 'w2' } },
      { method: 'get_diff', params: { session_id: 's1' } },
      { method: 'get_transcript', params: { session_id: 's1', limit: null } },
      { method: 'get_transcript', params: { session_id: 's1', limit: 20 } },
    ]);
  });

  it('turns a proto error into a DaemonError with its code', async () => {
    const h = fakeHost(() => ({ kind: 'error', data: { code: 'conflict', message: 'the process is still running; Retry re-runs a command that has exited' } }));
    const c = new RealClient({ transport: h.transport });
    const err = await c.retrySession('s1').catch((e: unknown) => e);
    expect(err).toBeInstanceOf(DaemonError);
    expect((err as DaemonError).code).toBe('conflict');
    expect((err as DaemonError).message).toContain('still running');
  });

  it('a transport failure is an `unavailable` DaemonError, and a failed connect is retried in the background', async () => {
    vi.useFakeTimers();
    const h = fakeHost();
    h.failConnect(1);
    const c = new RealClient({ transport: h.transport });
    const first = c.ping().catch((e: unknown) => e);
    await vi.advanceTimersByTimeAsync(0);
    const err = await first;
    expect(err).toBeInstanceOf(DaemonError);
    expect((err as DaemonError).code).toBe('unavailable');
    const states: string[] = [];
    c.onConnection((s) => states.push(s));
    await vi.advanceTimersByTimeAsync(600); // backoff elapsed: the retry connects
    expect(states).toContain('connected');
    await c.ping(); // usable again
    c.close();
  });

  it('forwards daemon events and link changes to subscribers; requests fail fast while the host reconnects', async () => {
    const h = fakeHost();
    const c = new RealClient({ transport: h.transport });
    await c.connect();
    const events: DaemonEvent[] = [];
    const states: string[] = [];
    c.onEvent((e) => events.push(e));
    c.onConnection((s) => states.push(s));
    h.emit('lotse://event', { event: 'session_closed', data: { session_id: 's9' } });
    expect(events).toEqual([{ event: 'session_closed', data: { session_id: 's9' } }]);
    h.emit('lotse://connection', 'disconnected');
    await expect(c.ping()).rejects.toMatchObject({ code: 'unavailable' });
    h.emit('lotse://connection', 'reconnected');
    await c.ping();
    expect(states).toEqual(['disconnected', 'reconnected']);
  });

  it('routes PTY messages to the session\'s handlers and queues bytes that arrive early', async () => {
    const h = fakeHost((r) => (r.method === 'attach' ? { kind: 'attached', data: { session_id: 's1', cols: 80, rows: 24, replay_len: 2 } } : { kind: 'ok' }));
    const c = new RealClient({ transport: h.transport });
    await c.connect();
    const attached = c.attach('s1', 80, 24);
    h.pty('s1', [104, 105]); // "hi": the replay arrives before any terminal subscribed
    const got: number[][] = [];
    const off = c.onPtyData('s1', (d) => got.push([...d]));
    await attached;
    expect(got).toEqual([[104, 105]]);
    h.pty('s1', [33]);
    h.pty('s2', [1]); // a session nobody attached: dropped, not queued
    expect(got).toEqual([[104, 105], [33]]);
    // the terminal goes away (session switch): in-flight bytes must not be replayed into the next terminal
    off();
    await c.detach('s1');
    h.pty('s1', [34]);
    const later: number[][] = [];
    c.onPtyData('s1', (d) => later.push([...d]));
    h.pty('s2', [2]);
    c.onPtyData('s2', (d) => later.push([...d]));
    expect(later).toEqual([]);
  });

  it('sends keystrokes as a raw body with the session id in a header', async () => {
    const h = fakeHost();
    const c = new RealClient({ transport: h.transport });
    await c.connect();
    c.writePty('s1', new Uint8Array([0x1b, 0x5b, 0x41]));
    await Promise.resolve();
    expect(h.raw).toEqual([{ command: 'daemon_pty_write', body: new Uint8Array([0x1b, 0x5b, 0x41]), headers: { 'x-lotse-session': 's1' } }]);
  });

  it('calls onReady once, after the first connection', async () => {
    const h = fakeHost();
    const onReady = vi.fn();
    const c = new RealClient({ transport: h.transport, onReady });
    await c.connect();
    await c.connect();
    expect(onReady).toHaveBeenCalledTimes(1);
    expect(onReady).toHaveBeenCalledWith(c);
  });
});

describe('createClient choice', () => {
  it('uses the mock in a browser and the real client in the app unless forced', () => {
    expect(chooseClient(undefined, false)).toBe('mock');
    expect(chooseClient(undefined, true)).toBe('real');
    expect(chooseClient('mock', true)).toBe('mock');
    expect(chooseClient('real', false)).toBe('real');
    expect(chooseClient('nonsense', true)).toBe('real');
  });
});
