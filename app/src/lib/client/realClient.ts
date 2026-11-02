import { Channel as TauriChannel } from '@tauri-apps/api/core';
import type { DaemonClient, ConnectionState, Unsubscribe } from './DaemonClient';
import type {
  AgentKind,
  AttachInfo,
  DaemonEvent,
  GitDiff,
  GitInfo,
  HelloOk,
  LaunchCommandInfo,
  NewProject,
  NewSession,
  PermDecision,
  Project,
  ProjectId,
  ProjectPatch,
  Request,
  Response,
  Session,
  SessionId,
  Settings,
  Snapshot,
  Transcript,
  Workspace,
  WorkspaceId,
} from './types';
import { DaemonError } from './types';

// The real `DaemonClient`: talks to `lotsed` through the Tauri bridge in `app/src-tauri/src/daemon.rs`.
//
//   requests   invoke('daemon_request', { request })  -> proto Response JSON ({kind, data})
//   events     Tauri event `lotse://event` (proto Event JSON), link changes on `lotse://connection`
//   PTY out    a Tauri Channel passed to `daemon_connect`; messages are `u16 BE id_len | id | bytes`
//   PTY in     invoke('daemon_pty_write', bytes, { headers: { 'x-lotse-session': id } })
//
// It connects on first use (the UI never calls `connect()`), keeps retrying in the background while the
// daemon is unreachable, and queues terminal bytes that arrive before a terminal subscribed.
// Selected by `createClient()` inside Tauri; `MockClient` stays the browser / `npm run dev` client.

/** What the client needs from Tauri; tests pass a fake. */
export interface Transport {
  invoke<T>(command: string, args?: Record<string, unknown>): Promise<T>;
  /** `invoke` with a raw (binary) body and headers. */
  invokeRaw(command: string, body: Uint8Array, headers: Record<string, string>): Promise<void>;
  listen<T>(event: string, handler: (payload: T) => void): Promise<() => void>;
  /** A channel the host pushes binary messages into. The returned value is passed to `invoke`. */
  channel(onMessage: (data: ArrayBuffer | number[]) => void): unknown;
}

export const tauriTransport: Transport = {
  async invoke<T>(command: string, args?: Record<string, unknown>) {
    const { invoke } = await import('@tauri-apps/api/core');
    return invoke<T>(command, args);
  },
  async invokeRaw(command, body, headers) {
    const { invoke } = await import('@tauri-apps/api/core');
    await invoke(command, body, { headers });
  },
  async listen<T>(event: string, handler: (payload: T) => void) {
    const { listen } = await import('@tauri-apps/api/event');
    return listen<T>(event, (e) => handler(e.payload));
  },
  channel(onMessage) {
    // Constructed synchronously: `Channel` is a plain class, imported eagerly by the bundler.
    const c = new TauriChannel<ArrayBuffer | number[]>();
    c.onmessage = onMessage;
    return c;
  },
};

const SESSION_HEADER = 'x-lotse-session';
/// Terminal bytes kept per session until a terminal subscribes (the replay can beat the subscription).
const BACKLOG_LIMIT = 8 * 1024 * 1024;
const RETRY_MS = [500, 1000, 2000, 5000];

export interface RealClientOptions {
  transport?: Transport;
  /** Called once after the first successful connection (native badge, focus reporting). */
  onReady?: (client: RealClient) => void;
}

type Handlers<T> = Set<(v: T) => void>;
type DataKind = Exclude<Response['kind'], 'ok' | 'pong' | 'error'>;
type DataOf<K extends DataKind> = Extract<Response, { kind: K }> extends { data: infer D } ? D : never;

export class RealClient implements DaemonClient {
  private readonly t: Transport;
  private readonly onReady?: (client: RealClient) => void;
  private readonly eventHandlers: Handlers<DaemonEvent> = new Set();
  private readonly connHandlers: Handlers<ConnectionState> = new Set();
  private readonly pty = new Map<SessionId, Handlers<Uint8Array>>();
  private readonly backlog = new Map<SessionId, { chunks: Uint8Array[]; bytes: number }>();
  /** Sessions with an `attach` in flight and no terminal subscribed yet: only these queue early bytes. */
  private readonly awaiting = new Set<SessionId>();
  private readonly decoder = new TextDecoder();
  private hello: HelloOk | null = null;
  private connecting: Promise<HelloOk> | null = null;
  private listening: Promise<void> | null = null;
  private stops: Array<() => void> = [];
  private everConnected = false;
  /** The host reported the link down; it is reconnecting. */
  private down = false;
  private retry = 0;
  private retryTimer: ReturnType<typeof setTimeout> | null = null;
  private closed = false;

  constructor(options: RealClientOptions = {}) {
    this.t = options.transport ?? tauriTransport;
    this.onReady = options.onReady;
  }

  // ---- connection ----------------------------------------------------------------------------

  connect(): Promise<HelloOk> {
    if (this.hello) return Promise.resolve(this.hello);
    this.connecting ??= this.doConnect().finally(() => {
      this.connecting = null;
    });
    return this.connecting;
  }

  private async doConnect(): Promise<HelloOk> {
    await this.listen();
    let hello: HelloOk;
    try {
      const channel = this.t.channel((data) => this.onPtyMessage(data));
      hello = await this.t.invoke<HelloOk>('daemon_connect', { pty: channel });
    } catch (e) {
      this.scheduleRetry();
      throw new DaemonError({ code: 'unavailable', message: String(e) });
    }
    this.hello = hello;
    this.retry = 0;
    if (!this.everConnected) {
      this.everConnected = true;
      this.emitConnection('connected');
      this.onReady?.(this);
    } else {
      this.emitConnection('reconnected');
    }
    return hello;
  }

  /** Subscribe to the host's events once, before the first request (nothing may be missed). */
  private listen(): Promise<void> {
    this.listening ??= (async () => {
      const offEvent = await this.t.listen<DaemonEvent>('lotse://event', (e) => {
        for (const h of [...this.eventHandlers]) h(e);
      });
      // The host reconnects (and restarts the daemon) by itself; requests fail fast meanwhile.
      const offConn = await this.t.listen<'disconnected' | 'reconnected'>('lotse://connection', (s) => {
        this.down = s === 'disconnected';
        this.backlog.clear();
        this.awaiting.clear();
        this.emitConnection(s);
      });
      this.stops.push(offEvent, offConn);
    })();
    return this.listening;
  }

  private scheduleRetry(): void {
    if (this.closed || this.retryTimer) return;
    const wait = RETRY_MS[Math.min(this.retry++, RETRY_MS.length - 1)] ?? 5000;
    this.retryTimer = setTimeout(() => {
      this.retryTimer = null;
      this.connect().catch(() => {});
    }, wait);
  }

  private emitConnection(state: ConnectionState): void {
    for (const h of [...this.connHandlers]) h(state);
  }

  private async ready(): Promise<void> {
    if (this.down) throw new DaemonError({ code: 'unavailable', message: 'lotsed is not running; reconnecting…' });
    if (!this.hello) await this.connect();
  }

  close(): void {
    this.closed = true;
    if (this.retryTimer) clearTimeout(this.retryTimer);
    this.stops.forEach((f) => f());
    this.stops = [];
  }

  // ---- requests --------------------------------------------------------------------------------

  private async call(request: Request): Promise<Response> {
    await this.ready();
    let response: Response;
    try {
      response = await this.t.invoke<Response>('daemon_request', { request });
    } catch (e) {
      throw new DaemonError({ code: 'unavailable', message: String(e) });
    }
    if (response.kind === 'error') throw new DaemonError(response.data);
    return response;
  }

  private async data<K extends DataKind>(request: Request, kind: K): Promise<DataOf<K>> {
    const r = await this.call(request);
    if (r.kind !== kind) throw new DaemonError({ code: 'internal', message: `expected ${kind}, got ${r.kind}` });
    return (r as { data: unknown }).data as DataOf<K>;
  }

  private async ok(request: Request): Promise<void> {
    await this.call(request);
  }

  async ping(): Promise<void> {
    await this.call({ method: 'ping' });
  }
  getSnapshot(): Promise<Snapshot> {
    return this.data({ method: 'get_snapshot' }, 'snapshot');
  }
  listProjects(): Promise<Project[]> {
    return this.data({ method: 'list_projects' }, 'projects');
  }
  addProject(project: NewProject): Promise<Project> {
    return this.data({ method: 'add_project', params: project }, 'project');
  }
  updateProject(projectId: ProjectId, patch: Partial<ProjectPatch>): Promise<Project> {
    return this.data({ method: 'update_project', params: { project_id: projectId, patch: patch as ProjectPatch } }, 'project');
  }
  removeProject(projectId: ProjectId): Promise<void> {
    return this.ok({ method: 'remove_project', params: { project_id: projectId } });
  }
  moveProject(projectId: ProjectId, workspaceId: WorkspaceId): Promise<Project> {
    return this.data({ method: 'move_project', params: { project_id: projectId, workspace_id: workspaceId } }, 'project');
  }
  createWorkspace(name: string): Promise<Workspace> {
    return this.data({ method: 'create_workspace', params: { name } }, 'workspace');
  }
  renameWorkspace(workspaceId: WorkspaceId, name: string): Promise<Workspace> {
    return this.data({ method: 'rename_workspace', params: { workspace_id: workspaceId, name } }, 'workspace');
  }
  deleteWorkspace(workspaceId: WorkspaceId, moveProjects: boolean): Promise<void> {
    return this.ok({ method: 'delete_workspace', params: { workspace_id: workspaceId, move_projects: moveProjects } });
  }
  listSessions(projectId?: ProjectId): Promise<Session[]> {
    return this.data({ method: 'list_sessions', params: { project_id: projectId ?? null } }, 'sessions');
  }
  createSession(session: NewSession): Promise<Session> {
    return this.data({ method: 'create_session', params: session }, 'session');
  }
  renameSession(sessionId: SessionId, title: string): Promise<void> {
    return this.ok({ method: 'rename_session', params: { session_id: sessionId, title } });
  }
  closeSession(sessionId: SessionId): Promise<void> {
    this.forget(sessionId);
    return this.ok({ method: 'close_session', params: { session_id: sessionId } });
  }
  async retrySession(sessionId: SessionId): Promise<void> {
    // The daemon answers `conflict` while the process still runs: the caller shows the message.
    await this.data({ method: 'retry_session', params: { session_id: sessionId } }, 'session');
  }
  markRead(sessionId: SessionId): Promise<void> {
    return this.ok({ method: 'mark_read', params: { session_id: sessionId } });
  }
  resolvePermission(sessionId: SessionId, decision: PermDecision): Promise<void> {
    return this.ok({ method: 'resolve_permission', params: { session_id: sessionId, decision } });
  }
  attach(sessionId: SessionId, cols: number, rows: number): Promise<AttachInfo> {
    // the replay can arrive before the answer: bytes are queued for this session until a terminal subscribes
    this.awaiting.add(sessionId);
    return this.data({ method: 'attach', params: { session_id: sessionId, cols, rows } }, 'attached');
  }
  detach(sessionId: SessionId): Promise<void> {
    this.forget(sessionId);
    return this.ok({ method: 'detach', params: { session_id: sessionId } });
  }
  /** Stop queueing terminal bytes for a session nobody is looking at (stale output would be replayed into the next terminal). */
  private forget(sessionId: SessionId): void {
    this.awaiting.delete(sessionId);
    this.backlog.delete(sessionId);
  }
  sendInput(sessionId: SessionId, text: string): Promise<void> {
    return this.ok({ method: 'send_input', params: { session_id: sessionId, text } });
  }
  resize(sessionId: SessionId, cols: number, rows: number): Promise<void> {
    return this.ok({ method: 'resize', params: { session_id: sessionId, cols, rows } });
  }
  getSettings(): Promise<Settings> {
    return this.data({ method: 'get_settings' }, 'settings');
  }
  updateSettings(settings: Settings): Promise<Settings> {
    return this.data({ method: 'update_settings', params: { settings } }, 'settings');
  }
  detectLaunchCommands(): Promise<LaunchCommandInfo[]> {
    return this.data({ method: 'detect_launch_commands' }, 'launch_commands');
  }
  getGitInfo(projectId: ProjectId, sessionId?: SessionId): Promise<GitInfo> {
    return this.data({ method: 'get_git_info', params: { project_id: projectId, session_id: sessionId ?? null } }, 'git_info');
  }
  getDiff(sessionId: SessionId): Promise<GitDiff> {
    return this.data({ method: 'get_diff', params: { session_id: sessionId } }, 'diff');
  }
  getTranscript(sessionId: SessionId, limit?: number): Promise<Transcript> {
    return this.data({ method: 'get_transcript', params: { session_id: sessionId, limit: limit ?? null } }, 'transcript');
  }
  hookEvent(agent: AgentKind, sessionId: SessionId | null, payload: unknown): Promise<void> {
    return this.ok({ method: 'hook_event', params: { agent, session_id: sessionId, payload } });
  }
  shutdown(): Promise<void> {
    return this.ok({ method: 'shutdown' });
  }

  // ---- subscriptions ---------------------------------------------------------------------------

  onConnection(handler: (state: ConnectionState) => void): Unsubscribe {
    this.connHandlers.add(handler);
    this.start();
    return () => this.connHandlers.delete(handler);
  }

  onEvent(handler: (event: DaemonEvent) => void): Unsubscribe {
    this.eventHandlers.add(handler);
    this.start();
    return () => this.eventHandlers.delete(handler);
  }

  /** The first subscriber starts the connection, so a fresh UI needs no explicit `connect()`. */
  private start(): void {
    if (!this.hello && !this.connecting && !this.closed) this.connect().catch(() => {});
  }

  onPtyData(sessionId: SessionId, handler: (data: Uint8Array) => void): Unsubscribe {
    const set = this.pty.get(sessionId) ?? new Set();
    this.pty.set(sessionId, set);
    set.add(handler);
    const queued = this.backlog.get(sessionId);
    this.awaiting.delete(sessionId);
    if (queued) {
      this.backlog.delete(sessionId);
      for (const chunk of queued.chunks) handler(chunk);
    }
    return () => {
      set.delete(handler);
      if (set.size === 0) this.pty.delete(sessionId);
    };
  }

  writePty(sessionId: SessionId, data: Uint8Array): void {
    this.t.invokeRaw('daemon_pty_write', data, { [SESSION_HEADER]: sessionId }).catch(() => {
      // the link is down: the terminal keeps its screen, input resumes after `reconnected`
    });
  }

  private onPtyMessage(raw: ArrayBuffer | number[]): void {
    const bytes = raw instanceof ArrayBuffer ? new Uint8Array(raw) : Uint8Array.from(raw);
    if (bytes.length < 2) return;
    const idLen = (bytes[0]! << 8) | bytes[1]!;
    if (bytes.length < 2 + idLen) return;
    const id = this.decoder.decode(bytes.subarray(2, 2 + idLen));
    const data = bytes.subarray(2 + idLen);
    const handlers = this.pty.get(id);
    if (handlers && handlers.size > 0) {
      for (const h of [...handlers]) h(data);
      return;
    }
    if (!this.awaiting.has(id)) return; // nobody asked for this session's output (detached, closed): drop it
    const q = this.backlog.get(id) ?? { chunks: [], bytes: 0 };
    q.chunks.push(data);
    q.bytes += data.length;
    while (q.bytes > BACKLOG_LIMIT && q.chunks.length > 1) q.bytes -= q.chunks.shift()!.length;
    this.backlog.set(id, q);
  }
}
