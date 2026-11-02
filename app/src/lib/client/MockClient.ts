import snapshotMessage from '../../../../crates/lotse-proto/fixtures/snapshot_mock.json';
import type { ConnectionState, DaemonClient, Unsubscribe } from './DaemonClient';
import { MOCK_PERM_NO, MOCK_PERM_OK, MOCK_TERMINAL, mockNewSessionScreen } from './mockTerminal';
import type {
  AgentKind,
  AttachInfo,
  DaemonEvent,
  GitInfo,
  HelloOk,
  LaunchCommandInfo,
  NewProject,
  NewSession,
  NotificationKind,
  PermDecision,
  PermRequest,
  Project,
  ProjectId,
  ProjectPatch,
  Session,
  SessionId,
  Settings,
  Snapshot,
  Status,
  SubAgent,
} from './types';
import { DaemonError, PROTO_VERSION } from './types';

/** `spec/mock.html` data model (`P`) as a proto snapshot: shared with the Rust fixture tests. */
const FIXTURE = (snapshotMessage as unknown as { body: { response: { data: Snapshot } } }).body.response.data;
/** The fixture's "now" (2026-11-02 09:14 UTC). MockClient shifts every timestamp so ages look the same today. */
export const FIXTURE_NOW = 1793610840000;

const LABEL: Record<AgentKind, string> = {
  terminal: 'Terminal',
  claude_code: 'Claude Code',
  codex: 'Codex',
  opencode: 'OpenCode',
};
const SUBTITLE: Record<NotificationKind, string> = {
  perm: 'Needs permission',
  fail: 'Failed',
  done: 'Done',
  sub_agent_done: 'Sub-agent done',
};
const SWATCHES = ['purple', 'indigo', 'teal', 'sky', 'lavender', 'tan', 'sage', 'yellow'];

const clone = <T>(v: T): T => JSON.parse(JSON.stringify(v)) as T;
const enc = new TextEncoder();

export interface ScriptStep {
  /** Milliseconds of mock time after start. */
  at: number;
  run: (c: MockClient) => void;
}

/** Scripted status changes, one pass: finish, start, fail, sub-agent done, permission request, auto-allow. */
export const DEFAULT_SCRIPT: ScriptStep[] = [
  { at: 4_000, run: (c) => c.setStatus('a5', 'done') },
  { at: 9_000, run: (c) => c.setStatus('a2', 'run') },
  { at: 16_000, run: (c) => c.setStatus('a2', 'fail') },
  { at: 22_000, run: (c) => c.setSubAgent('a1', 'a1-s2', 'done') },
  {
    at: 28_000,
    run: (c) => c.setStatus('a1', 'perm', { tool: 'Bash', summary: 'npm test -- totals' }),
  },
  { at: 40_000, run: (c) => void c.resolvePermission('a1', 'allow') },
];

/**
 * Sub-agents start and stop on a Claude Code (a1), a Codex (a2) and an OpenCode (a6) session: the list
 * arrives as `sub_agents_changed`, each stop as `sub_agent_status`. Use as `script: SUB_AGENT_SCRIPT`.
 */
export const SUB_AGENT_SCRIPT: ScriptStep[] = [
  { at: 1_000, run: (c) => c.setSubAgents('a1', [...c.subAgentsOf('a1'), { id: 'a1-s3', name: 'lint-fixer', status: 'run' }]) },
  { at: 2_000, run: (c) => c.setSubAgents('a2', [{ id: 'a2-s1', name: 'schema-check', status: 'run' }]) },
  { at: 3_000, run: (c) => c.setSubAgents('a6', [{ id: 'a6-s1', name: 'docs-scan', status: 'run' }]) },
  { at: 5_000, run: (c) => c.setSubAgent('a1', 'a1-s3', 'done') },
  { at: 6_000, run: (c) => c.setSubAgent('a2', 'a2-s1', 'done') },
  { at: 7_000, run: (c) => c.setSubAgent('a6', 'a6-s1', 'done') },
];

export interface MockClientOptions {
  /** Clock, default `Date.now`. */
  now?: () => number;
  /** Scripted status changes: `true` (default, `DEFAULT_SCRIPT`), `false` (static), or custom steps. */
  script?: boolean | ScriptStep[];
  /** Real-time tick for the script, ms. `0` disables the timer (tests call `tick`). Default 1000. */
  tickMs?: number;
}

/** In-memory `DaemonClient` seeded with the mock's projects, sessions, sub-agents and statuses. */
export class MockClient implements DaemonClient {
  private readonly now: () => number;
  private projects: Project[];
  private sessions: Session[];
  private settings: Settings;
  private readonly daemon: Snapshot['daemon'];
  private readonly listeners = new Set<(e: DaemonEvent) => void>();
  private readonly connListeners = new Set<(s: ConnectionState) => void>();
  private readonly pty = new Map<SessionId, Set<(d: Uint8Array) => void>>();
  private readonly screens = new Map<SessionId, string>();
  private readonly steps: ScriptStep[];
  private nextStep = 0;
  private elapsed = 0;
  private nextSession = 8;
  private nextProject = 6;
  private timer: ReturnType<typeof setInterval> | null = null;

  constructor(opts: MockClientOptions = {}) {
    this.now = opts.now ?? Date.now;
    const delta = this.now() - FIXTURE_NOW;
    const snap = clone(FIXTURE);
    this.projects = snap.projects;
    this.settings = snap.settings;
    this.daemon = { ...snap.daemon, started_at: snap.daemon.started_at + delta };
    this.sessions = snap.sessions.map((s) => ({
      ...s,
      created_at: s.created_at + delta,
      last_activity: s.last_activity + delta,
      running_since: s.running_since === null ? null : s.running_since + delta,
    }));
    const script = opts.script ?? true;
    this.steps = script === true ? DEFAULT_SCRIPT : script === false ? [] : script;
    const tickMs = opts.tickMs ?? 1000;
    if (this.steps.length > 0 && tickMs > 0) {
      this.timer = setInterval(() => this.tick(tickMs), tickMs);
    }
  }

  // --- script ----------------------------------------------------------------------------------

  /** Advance mock time and run the script steps that became due. */
  tick(dtMs: number): void {
    this.elapsed += dtMs;
    while (this.nextStep < this.steps.length && (this.steps[this.nextStep]?.at ?? Infinity) <= this.elapsed) {
      this.steps[this.nextStep++]?.run(this);
    }
  }

  /** Set a session's status (what the daemon would do on a detection), emit events and notifications. */
  setStatus(sessionId: SessionId, status: Status, perm: PermRequest | null = null): void {
    const s = this.must(sessionId);
    const at = this.now();
    const was = s.status;
    s.status = status;
    s.perm = status === 'perm' ? perm : null;
    s.running_since = status === 'run' ? (was === 'run' ? s.running_since : at) : null;
    s.last_activity = at;
    const becameUnread = status === 'done' && !s.unread;
    if (status === 'done') s.unread = true;
    const sub = status === 'perm' ? s.sub_agents.find((x) => x.status === 'run') : undefined;
    if (sub) sub.status = 'perm';
    this.emit({
      event: 'status_changed',
      data: { session_id: s.id, status, source: s.source, perm: s.perm, running_since: s.running_since, at },
    });
    // Like the daemon: sub-agent list and the unread flag travel in their own events.
    if (sub) this.emit({ event: 'sub_agents_changed', data: { session_id: s.id, sub_agents: clone(s.sub_agents) } });
    if (becameUnread) this.emit({ event: 'session_updated', data: clone(s) });
    if (status === 'perm' || status === 'fail' || status === 'done') {
      this.notify(s, status, status === 'perm' && s.perm ? `${s.perm.tool}(${s.perm.summary})` : null);
    }
  }

  /** Replace a session's sub-agent list (what the daemon sends when sub-agents start or stop). */
  setSubAgents(sessionId: SessionId, subAgents: SubAgent[]): void {
    const s = this.must(sessionId);
    s.sub_agents = clone(subAgents);
    s.last_activity = this.now();
    this.emit({ event: 'sub_agents_changed', data: { session_id: s.id, sub_agents: clone(s.sub_agents) } });
  }

  subAgentsOf(sessionId: SessionId): SubAgent[] {
    return clone(this.must(sessionId).sub_agents);
  }

  setSubAgent(sessionId: SessionId, subAgentId: string, status: Status): void {
    const s = this.must(sessionId);
    const sub = s.sub_agents.find((x) => x.id === subAgentId);
    if (!sub) throw new DaemonError({ code: 'not_found', message: `no sub-agent ${subAgentId}` });
    sub.status = status;
    s.last_activity = this.now();
    this.emit({ event: 'sub_agent_status', data: { session_id: s.id, sub_agent_id: sub.id, status } });
    if (status === 'done') this.notify(s, 'sub_agent_done', sub.name);
  }

  // --- DaemonClient ----------------------------------------------------------------------------

  async connect(): Promise<HelloOk> {
    this.setConnection('connected');
    return { proto_version: PROTO_VERSION, daemon_version: this.daemon.version };
  }

  /** Simulate the link dropping and coming back (`disconnected`, then `reconnected`). */
  setConnection(state: ConnectionState): void {
    for (const l of [...this.connListeners]) l(state);
  }

  close(): void {
    if (this.timer) clearInterval(this.timer);
    this.timer = null;
    this.listeners.clear();
    this.connListeners.clear();
    this.pty.clear();
  }

  async ping(): Promise<void> {}

  async getSnapshot(): Promise<Snapshot> {
    return clone({ daemon: this.daemon, projects: this.projects, sessions: this.sessions, settings: this.settings });
  }

  async listProjects(): Promise<Project[]> {
    return clone(this.projects);
  }

  async addProject(p: NewProject): Promise<Project> {
    const name = p.name ?? (p.path.replace(/\/+$/, '').split('/').pop() || p.path);
    const project: Project = {
      id: `p${this.nextProject++}`,
      workspace_id: 'w1',
      name,
      path: p.path,
      color: p.color ?? SWATCHES[this.projects.length % SWATCHES.length] ?? 'purple',
      icon: p.icon ?? 'folder',
      is_git: true,
      notifications: { mode: 'global' },
    };
    this.projects.push(project);
    this.emit({ event: 'project_added', data: clone(project) });
    return clone(project);
  }

  async updateProject(projectId: ProjectId, patch: Partial<ProjectPatch>): Promise<Project> {
    const p = this.mustProject(projectId);
    if (patch.name != null) p.name = patch.name;
    if (patch.color != null) p.color = patch.color;
    if (patch.icon != null) p.icon = patch.icon;
    if (patch.notifications != null) p.notifications = patch.notifications;
    this.emit({ event: 'project_updated', data: clone(p) });
    return clone(p);
  }

  async removeProject(projectId: ProjectId): Promise<void> {
    this.mustProject(projectId);
    for (const s of this.sessions.filter((x) => x.project_id === projectId)) await this.closeSession(s.id);
    this.projects = this.projects.filter((p) => p.id !== projectId);
    this.emit({ event: 'project_removed', data: { project_id: projectId } });
  }

  async listSessions(projectId?: ProjectId): Promise<Session[]> {
    return clone(projectId ? this.sessions.filter((s) => s.project_id === projectId) : this.sessions);
  }

  async createSession(n: NewSession): Promise<Session> {
    const project = this.mustProject(n.project_id);
    const at = this.now();
    const git: GitInfo = project.is_git
      ? {
          is_git: true,
          branch: n.worktree?.branch ?? 'main',
          ahead: 0,
          dirty: 0,
          worktree: n.worktree !== null,
          worktree_path: n.worktree
            ? `~/.lotse/worktrees/${project.name}/${n.worktree.branch.replace(/^lotse\//, '')}`
            : null,
        }
      : { is_git: false, branch: '', ahead: 0, dirty: 0, worktree: false, worktree_path: null };
    const session: Session = {
      id: `a${this.nextSession++}`,
      project_id: project.id,
      title: n.title ?? LABEL[n.agent],
      agent: n.agent,
      command: n.command ?? this.settings.launch_commands[n.agent],
      status: n.agent === 'terminal' ? 'idle' : 'unknown',
      sub_agents: [],
      git,
      created_at: at,
      last_activity: at,
      running_since: null,
      unread: false,
      perm: null,
      source: 'process',
    };
    this.sessions.push(session);
    this.emit({ event: 'session_created', data: clone(session) });
    return clone(session);
  }

  async renameSession(sessionId: SessionId, title: string): Promise<void> {
    const s = this.must(sessionId);
    s.title = title;
    this.emit({ event: 'session_updated', data: clone(s) });
  }

  async closeSession(sessionId: SessionId): Promise<void> {
    this.must(sessionId);
    this.sessions = this.sessions.filter((s) => s.id !== sessionId);
    this.pty.delete(sessionId);
    this.emit({ event: 'session_closed', data: { session_id: sessionId } });
  }

  async retrySession(sessionId: SessionId): Promise<void> {
    this.must(sessionId);
    this.setStatus(sessionId, 'run');
  }

  async markRead(sessionId: SessionId): Promise<void> {
    const s = this.must(sessionId);
    if (!s.unread) return;
    s.unread = false;
    this.emit({ event: 'session_updated', data: clone(s) });
  }

  async resolvePermission(sessionId: SessionId, decision: PermDecision): Promise<void> {
    const s = this.must(sessionId);
    const allowed = decision !== 'deny';
    for (const sub of s.sub_agents) if (sub.status === 'perm') sub.status = allowed ? 'run' : 'idle';
    if (s.sub_agents.length > 0) {
      this.emit({ event: 'sub_agents_changed', data: { session_id: s.id, sub_agents: clone(s.sub_agents) } });
    }
    this.setStatus(sessionId, allowed ? 'run' : 'idle');
    if (sessionId === 'a3') this.show('a3', allowed ? MOCK_PERM_OK : MOCK_PERM_NO);
  }

  async attach(sessionId: SessionId, cols: number, rows: number): Promise<AttachInfo> {
    const s = this.must(sessionId);
    const screen = this.screens.get(sessionId) ?? MOCK_TERMINAL[sessionId] ?? mockNewSessionScreen(LABEL[s.agent], '~');
    const bytes = enc.encode(screen);
    setTimeout(() => this.pty.get(sessionId)?.forEach((h) => h(bytes)), 0);
    return { session_id: sessionId, cols, rows, replay_len: bytes.length };
  }

  async detach(sessionId: SessionId): Promise<void> {
    this.must(sessionId);
  }

  async sendInput(sessionId: SessionId, text: string): Promise<void> {
    this.must(sessionId);
    this.writePty(sessionId, enc.encode(text));
  }

  async resize(sessionId: SessionId): Promise<void> {
    this.must(sessionId);
  }

  async getSettings(): Promise<Settings> {
    return clone(this.settings);
  }

  async updateSettings(settings: Settings): Promise<Settings> {
    this.settings = clone(settings);
    this.emit({ event: 'settings_changed', data: clone(this.settings) });
    return clone(this.settings);
  }

  async detectLaunchCommands(): Promise<LaunchCommandInfo[]> {
    const facts: Record<AgentKind, Pick<LaunchCommandInfo, 'found' | 'resolved_path' | 'version' | 'integration'>> = {
      terminal: { found: true, resolved_path: '/bin/zsh', version: 'zsh 5.9', integration: null },
      claude_code: { found: true, resolved_path: '/opt/homebrew/bin/claude', version: 'v2.1.309', integration: 'hooks installed' },
      codex: { found: true, resolved_path: '/opt/homebrew/bin/codex', version: 'v0.98', integration: 'OSC 7501' },
      opencode: { found: false, resolved_path: null, version: null, integration: null },
    };
    const agents: AgentKind[] = ['terminal', 'claude_code', 'codex', 'opencode'];
    return agents.map((agent) => ({
      agent,
      default_command: agent === 'terminal' ? '$SHELL -l' : agent === 'claude_code' ? 'claude' : agent,
      command: this.settings.launch_commands[agent],
      ...facts[agent],
    }));
  }

  async getGitInfo(projectId: ProjectId, sessionId?: SessionId): Promise<GitInfo> {
    const p = this.mustProject(projectId);
    const s = sessionId ? this.must(sessionId) : this.sessions.find((x) => x.project_id === p.id);
    if (!p.is_git || !s) return { is_git: p.is_git, branch: p.is_git ? 'main' : '', ahead: 0, dirty: 0, worktree: false, worktree_path: null };
    return clone(s.git);
  }

  async hookEvent(): Promise<void> {}

  async shutdown(): Promise<void> {
    this.close();
  }

  onConnection(handler: (state: ConnectionState) => void): Unsubscribe {
    this.connListeners.add(handler);
    return () => this.connListeners.delete(handler);
  }

  onEvent(handler: (event: DaemonEvent) => void): Unsubscribe {
    this.listeners.add(handler);
    return () => this.listeners.delete(handler);
  }

  onPtyData(sessionId: SessionId, handler: (data: Uint8Array) => void): Unsubscribe {
    const set = this.pty.get(sessionId) ?? new Set();
    set.add(handler);
    this.pty.set(sessionId, set);
    return () => set.delete(handler);
  }

  /** Local echo: `\r` becomes `\r\n`, enough for a typing demo. */
  writePty(sessionId: SessionId, data: Uint8Array): void {
    const text = new TextDecoder().decode(data).replace(/\r/g, '\r\n');
    this.pty.get(sessionId)?.forEach((h) => h(enc.encode(text)));
  }

  // --- internals -------------------------------------------------------------------------------

  private emit(e: DaemonEvent): void {
    for (const l of [...this.listeners]) l(e);
  }

  private show(sessionId: SessionId, screen: string): void {
    this.screens.set(sessionId, screen);
    this.pty.get(sessionId)?.forEach((h) => h(enc.encode('\x1b[2J\x1b[H' + screen)));
  }

  private must(id: SessionId): Session {
    const s = this.sessions.find((x) => x.id === id);
    if (!s) throw new DaemonError({ code: 'not_found', message: `no session ${id}` });
    return s;
  }

  private mustProject(id: ProjectId): Project {
    const p = this.projects.find((x) => x.id === id);
    if (!p) throw new DaemonError({ code: 'not_found', message: `no project ${id}` });
    return p;
  }

  /** Daemon-side notification decision: settings table, pause toggle, per-project override. */
  private notify(s: Session, kind: NotificationKind, body: string | null): void {
    const n = this.settings.notifications;
    if (n.paused) return;
    const project = this.mustProject(s.project_id);
    const mode = project.notifications;
    const wanted =
      mode.mode === 'mute' ? false : mode.mode === 'custom' ? mode[kind] : n[kind].banner;
    if (!wanted) return;
    this.emit({
      event: 'notification',
      data: {
        session_id: s.id,
        kind,
        title: `${project.name} · ${LABEL[s.agent]}`,
        subtitle: SUBTITLE[kind],
        body,
        sound: n[kind].sound,
        at: this.now(),
      },
    });
  }
}
