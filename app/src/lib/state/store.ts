import { useSyncExternalStore } from 'react';
import type { DaemonClient, Unsubscribe } from '../client/DaemonClient';
import { useClient } from '../client/context';
import type { DaemonEvent, DaemonInfo, Project, ProjectId, Session, SessionId, Settings } from '../client/types';
import { getSelectedSessionId, selectSession, subscribeSelection } from '../ui/selection';
import { groupSessions, mostUrgent, needYouCount } from './selectors';

// Workspace store: the snapshot (projects, sessions, settings) kept current by daemon events.
// One store per client (WeakMap), started with its first subscriber and stopped with its last, so the
// sidebar, status bar and shortcuts share one copy and React StrictMode remounts are harmless.
// It also owns the selection rules: pick an initial session, repair the selection when its session
// closes, mark the selected session read.

export interface WorkspaceState {
  /** First snapshot arrived. */
  loaded: boolean;
  connection: 'connected' | 'disconnected';
  daemon: DaemonInfo | null;
  projects: Project[];
  sessions: Session[];
  settings: Settings | null;
}

export const INITIAL_STATE: WorkspaceState = {
  loaded: false,
  connection: 'connected',
  daemon: null,
  projects: [],
  sessions: [],
  settings: null,
};

const patchSession = (state: WorkspaceState, id: SessionId, patch: (s: Session) => Session): WorkspaceState => {
  if (!state.sessions.some((s) => s.id === id)) return state;
  return { ...state, sessions: state.sessions.map((s) => (s.id === id ? patch(s) : s)) };
};

/** Apply one daemon event. Pure; returns the same object when the event changes nothing. */
export function reduceEvent(state: WorkspaceState, e: DaemonEvent): WorkspaceState {
  switch (e.event) {
    case 'project_added':
      return state.projects.some((p) => p.id === e.data.id) ? state : { ...state, projects: [...state.projects, e.data] };
    case 'project_updated':
      return { ...state, projects: state.projects.map((p) => (p.id === e.data.id ? e.data : p)) };
    case 'project_removed':
      return {
        ...state,
        projects: state.projects.filter((p) => p.id !== e.data.project_id),
        sessions: state.sessions.filter((s) => s.project_id !== e.data.project_id),
      };
    case 'session_created':
      return state.sessions.some((s) => s.id === e.data.id) ? state : { ...state, sessions: [...state.sessions, e.data] };
    case 'session_updated':
      return patchSession(state, e.data.id, () => e.data);
    case 'session_closed':
      return { ...state, sessions: state.sessions.filter((s) => s.id !== e.data.session_id) };
    case 'status_changed': {
      const d = e.data;
      return patchSession(state, d.session_id, (s) => ({
        ...s,
        status: d.status,
        source: d.source,
        perm: d.perm,
        running_since: d.running_since,
        last_activity: d.at,
      }));
    }
    case 'sub_agents_changed':
      return patchSession(state, e.data.session_id, (s) => ({ ...s, sub_agents: e.data.sub_agents }));
    case 'sub_agent_status': {
      const d = e.data;
      return patchSession(state, d.session_id, (s) => ({
        ...s,
        sub_agents: s.sub_agents.map((x) => (x.id === d.sub_agent_id ? { ...x, status: d.status } : x)),
      }));
    }
    case 'git_info_changed':
      return patchSession(state, e.data.session_id, (s) => ({ ...s, git: e.data.git }));
    case 'settings_changed':
      return { ...state, settings: e.data };
    case 'session_exited':
    case 'notification':
      return state;
  }
}

export class WorkspaceStore {
  private state: WorkspaceState = INITIAL_STATE;
  private readonly listeners = new Set<() => void>();
  private stops: Unsubscribe[] = [];
  /** Events that arrive while a snapshot is in flight; replayed on top of it. */
  private queue: DaemonEvent[] | null = null;
  private generation = 0;

  constructor(private readonly client: DaemonClient) {}

  getState = (): WorkspaceState => this.state;

  subscribe = (listener: () => void): Unsubscribe => {
    this.listeners.add(listener);
    if (this.listeners.size === 1) this.start();
    return () => {
      this.listeners.delete(listener);
      if (this.listeners.size === 0) this.stop();
    };
  };

  private start(): void {
    this.generation++;
    this.queue = [];
    if (import.meta.env.DEV && typeof window !== 'undefined') {
      // Dev-only handle so Playwright can seed many projects against the MockClient.
      const w = window as unknown as { __lotse?: { client?: DaemonClient } };
      w.__lotse = { ...w.__lotse, client: this.client };
    }
    this.stops = [
      this.client.onEvent((e) => (this.queue ? this.queue.push(e) : this.commit(reduceEvent(this.state, e)))),
      this.client.onConnection((c) => {
        if (c === 'disconnected') this.commit({ ...this.state, connection: 'disconnected' });
        else void this.load(); // connected / reconnected: events were missed, refetch
      }),
      subscribeSelection(() => this.reconcile()),
    ];
    void this.load();
  }

  private stop(): void {
    this.generation++;
    this.queue = null;
    this.stops.forEach((f) => f());
    this.stops = [];
  }

  private async load(): Promise<void> {
    const gen = ++this.generation;
    this.queue = [];
    try {
      const snap = await this.client.getSnapshot();
      if (gen !== this.generation) return;
      let next: WorkspaceState = {
        loaded: true,
        connection: 'connected',
        daemon: snap.daemon,
        projects: snap.projects,
        sessions: snap.sessions,
        settings: snap.settings,
      };
      for (const e of this.queue ?? []) next = reduceEvent(next, e);
      this.queue = null;
      this.commit(next);
    } catch {
      if (gen !== this.generation) return;
      this.queue = null;
      this.commit({ ...this.state, connection: 'disconnected' });
    }
  }

  private commit(next: WorkspaceState): void {
    if (next === this.state) return;
    const prev = this.state;
    this.state = next;
    if (next.loaded) {
      this.repairSelection(prev);
      if (getUiState().needFilter && needYouCount(next.sessions) === 0) setNeedFilter(false);
    }
    this.reconcile();
    for (const l of [...this.listeners]) l();
  }

  /** Initial selection, and a neighbour when the selected session closed. */
  private repairSelection(prev: WorkspaceState): void {
    const sessions = this.state.sessions;
    const sel = getSelectedSessionId();
    if (sel !== null && sessions.some((s) => s.id === sel)) return;
    const projectId = prev.sessions.find((s) => s.id === sel)?.project_id;
    const sibling = projectId ? sessions.find((s) => s.project_id === projectId) : undefined;
    const first = groupSessions(this.state.projects, sessions).flatMap((g) => g.sessions)[0];
    selectSession((sibling ?? first)?.id ?? null);
  }

  /** The selected session is being looked at: mark it read. */
  private reconcile(): void {
    const sel = getSelectedSessionId();
    const s = sel === null ? undefined : this.state.sessions.find((x) => x.id === sel);
    if (s?.unread) this.client.markRead(s.id).catch(() => {});
  }
}

const stores = new WeakMap<DaemonClient, WorkspaceStore>();

export function getStore(client: DaemonClient): WorkspaceStore {
  let store = stores.get(client);
  if (!store) {
    store = new WorkspaceStore(client);
    stores.set(client, store);
  }
  return store;
}

export const useWorkspaceStore = (): WorkspaceStore => getStore(useClient());

export function useWorkspace(): WorkspaceState {
  const store = useWorkspaceStore();
  return useSyncExternalStore(store.subscribe, store.getState);
}

/** Sessions that need you (permission or failed, own or sub-agent): the Dock-badge count. */
export function useNeedYouCount(): number {
  const { sessions } = useWorkspace();
  return needYouCount(sessions);
}

/**
 * Dock-badge source: calls `handler` with the need-you count now and on every change.
 * WP8/WP7 wire it to the native badge (`setBadgeCount`); the UI does not touch the Dock itself.
 */
export function subscribeNeedYouCount(client: DaemonClient, handler: (count: number) => void): Unsubscribe {
  const store = getStore(client);
  let last = -1;
  const check = () => {
    const state = store.getState();
    if (!state.loaded) return;
    const n = needYouCount(state.sessions);
    if (n !== last) {
      last = n;
      handler(n);
    }
  };
  const off = store.subscribe(check);
  check();
  return off;
}

// --- UI state shared by sidebar and status bar --------------------------------------------------

const COLLAPSED_KEY = 'lotse-collapsed';

export interface UiState {
  /** "▲ n need you" filter on. */
  needFilter: boolean;
  /** Projects the user collapsed (persisted). Everything else is expanded. */
  collapsed: ReadonlySet<ProjectId>;
}

function loadCollapsed(): Set<ProjectId> {
  try {
    const raw: unknown = JSON.parse(localStorage.getItem(COLLAPSED_KEY) ?? '[]');
    return new Set(Array.isArray(raw) ? raw.filter((x): x is string => typeof x === 'string') : []);
  } catch {
    return new Set();
  }
}

let ui: UiState = { needFilter: false, collapsed: loadCollapsed() };
const uiListeners = new Set<() => void>();

const setUi = (next: UiState): void => {
  ui = next;
  for (const l of [...uiListeners]) l();
};

export const getUiState = (): UiState => ui;

export function subscribeUi(listener: () => void): Unsubscribe {
  uiListeners.add(listener);
  return () => uiListeners.delete(listener);
}

export const useUiState = (): UiState => useSyncExternalStore(subscribeUi, getUiState);

export function setNeedFilter(on: boolean): void {
  if (ui.needFilter !== on) setUi({ ...ui, needFilter: on });
}

/** Footer chip / "Show all": toggling on jumps to the most urgent session, like the mock. */
export function toggleNeedFilter(sessions: readonly Session[]): void {
  if (ui.needFilter) return setNeedFilter(false);
  if (needYouCount(sessions) === 0) return;
  setNeedFilter(true);
  const id = mostUrgent(sessions);
  if (id) selectSession(id);
}

export function toggleCollapsed(projectId: ProjectId, collapse?: boolean): void {
  const next = new Set(ui.collapsed);
  const want = collapse ?? !next.has(projectId);
  if (want === next.has(projectId)) return;
  if (want) next.add(projectId);
  else next.delete(projectId);
  try {
    localStorage.setItem(COLLAPSED_KEY, JSON.stringify([...next]));
  } catch {
    // storage unavailable
  }
  setUi({ ...ui, collapsed: next });
}

/** Tests: drop in-memory UI state and reload it from storage. */
export function resetUiState(): void {
  setUi({ needFilter: false, collapsed: loadCollapsed() });
}
