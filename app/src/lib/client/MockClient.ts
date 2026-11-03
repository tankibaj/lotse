import snapshotMessage from '../../../../crates/lotse-proto/fixtures/snapshot_mock.json';
import type { ConnectionState, DaemonClient, Unsubscribe } from './DaemonClient';
import { MOCK_PERM_NO, MOCK_PERM_OK, MOCK_TERMINAL, mockNewSessionScreen } from './mockTerminal';
import type {
  AgentKind,
  AttachInfo,
  BackgroundTask,
  Connector,
  ConnectorStatus,
  DaemonEvent,
  DiffBase,
  DiffFile,
  DiffStatus,
  Export,
  ExportFormat,
  FileContent,
  FileEntry,
  FileTree,
  GitDiff,
  GitInfo,
  HelloOk,
  LaunchCommandInfo,
  NewProject,
  NewSession,
  NotificationKind,
  PermDecision,
  PermRequest,
  PermVia,
  Project,
  ProjectId,
  ProjectPatch,
  Review,
  Session,
  SessionId,
  SessionInfo,
  Settings,
  Snapshot,
  Status,
  SubAgent,
  Transcript,
  TranscriptTurn,
  Usage,
  UsageToday,
  Workspace,
  WorkspaceId,
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

const NO_GIT: GitInfo = { is_git: false, branch: '', ahead: 0, dirty: 0, worktree: false, worktree_path: null, behind: 0, added: 0, removed: 0, pr: null };

/** Lines a unified diff adds (`+`) or removes (`-`), without the `+++` / `---` headers. */
const countLines = (diff: string, sign: '+' | '-'): number =>
  diff.split('\n').filter((l) => l.startsWith(sign) && !l.startsWith(sign.repeat(3))).length;

const MOCK_DIFF_FILES: Omit<DiffFile, 'added' | 'removed'>[] = [
  {
    path: 'src/invoice.ts',
    old_path: null,
    status: 'modified',
    binary: false,
    truncated: false,
    diff:
      'diff --git a/src/invoice.ts b/src/invoice.ts\nindex 1a2b3c4..5d6e7f8 100644\n--- a/src/invoice.ts\n+++ b/src/invoice.ts\n@@ -10,7 +10,7 @@ export function total(lines: Line[]) {\n   let sum = 0;\n   for (const l of lines) {\n-    sum += Math.round(l.net * 100) / 100;\n+    sum += roundHalfEven(l.net);\n   }\n   return sum;\n }\n@@ -41,3 +41,4 @@ export function vat(net: number) {\n   return net * 0.19;\n }\n+export const ROUNDING = "half-even";\n',
  },
  {
    path: 'src/rounding.ts',
    old_path: null,
    status: 'added',
    binary: false,
    truncated: false,
    diff:
      'diff --git a/src/rounding.ts b/src/rounding.ts\nnew file mode 100644\nindex 0000000..9f8e7d6\n--- /dev/null\n+++ b/src/rounding.ts\n@@ -0,0 +1,3 @@\n+export function roundHalfEven(x: number) {\n+  return Math.round(x * 100) / 100;\n+}\n',
  },
  { path: 'docs/logo.png', old_path: null, status: 'modified', binary: true, truncated: false, diff: '' },
  { path: 'notes.md', old_path: null, status: 'untracked', binary: false, truncated: false, diff: '' },
];

/** What Show Changes lists for any mock session with uncommitted changes. */
const MOCK_DIFF: GitDiff = {
  is_git: true,
  truncated: false,
  files: MOCK_DIFF_FILES.map((f) => ({ ...f, added: countLines(f.diff, '+'), removed: countLines(f.diff, '-') })),
};

/**
 * What Show Transcript lists for a mock Claude Code or Codex session. The turns, their order and texts are
 * pinned by `ReadOnlySheets.test.tsx`; the v4 tool status and result ride along.
 * TODO(beta-wp15): switch to the `spec/mock.html` TRX turns (approvals, sub-agent tasks) with the new panel.
 */
const MOCK_TURNS: Omit<TranscriptTurn, 'at'>[] = [
  { role: 'user', text: 'Fix the invoice rounding so totals match the PDF.', tool: null },
  { role: 'assistant', text: "I'll start by looking at how line totals are summed.", tool: null },
  { role: 'tool', text: '', tool: { name: 'Grep', summary: 'roundTo', status: 'done', result: '3 call sites in totals.ts' } },
  { role: 'tool', text: '', tool: { name: 'Edit', summary: 'src/invoice.ts', status: 'done', result: '+2 −1' } },
  { role: 'assistant', text: 'Totals now round half-even per line. Running the tests.', tool: null },
  { role: 'tool', text: '', tool: { name: 'Bash', summary: 'npm test -- totals', status: 'run' } },
];

/** The commits the Changes panel lists, newest first (`spec/mock.html`); more are made up past these. */
const MOCK_COMMITS = [
  { sha: 'a41c9e2', subject: "Use banker's rounding for invoice totals", minutesAgo: 4 },
  { sha: '7be03d1', subject: 'Add roundHalfEven helper', minutesAgo: 11 },
];

/** The Files panel's tree (`spec/mock.html` FTREE); a trailing `/` marks a folder. */
const MOCK_TREE = [
  'src/',
  'src/invoice/',
  'src/invoice/totals.ts',
  'src/invoice/round.ts',
  'src/invoice/tax.ts',
  'src/api/',
  'src/api/routes.ts',
  'src/index.ts',
  'test/',
  'test/totals.spec.ts',
  'test/round.spec.ts',
  'package.json',
  'tsconfig.json',
  'README.md',
];
/** Changed files of a session with uncommitted changes (`spec/mock.html` DIFF). */
const MOCK_CHANGES: Record<string, { status: DiffStatus; added: number; removed: number }> = {
  'src/invoice/totals.ts': { status: 'modified', added: 2, removed: 1 },
  'src/invoice/round.ts': { status: 'added', added: 7, removed: 0 },
  'test/totals.spec.ts': { status: 'modified', added: 4, removed: 1 },
};
const MOCK_SOURCES: Record<string, string> = {
  'src/invoice/totals.ts': `import { Line, Invoice } from './types'
import { roundHalfEven } from './round'
import { taxFor } from './tax'

export interface Totals {
  net: number
  tax: number
  gross: number
}

/** Sum of a single line before tax. */
export function lineTotal(l: Line): number {
  return l.qty * l.price
}

export function totals(inv: Invoice): Totals {
  const net = invoiceTotal(inv.lines)
  const tax = roundHalfEven(taxFor(inv.country, net), 2)
  return { net, tax, gross: roundHalfEven(net + tax, 2) }
}

export function applyDiscount(t: Totals, pct: number): Totals {
  if (pct <= 0) return t
  const f = 1 - pct / 100
  return {
    net: roundHalfEven(t.net * f, 2),
    tax: roundHalfEven(t.tax * f, 2),
    gross: roundHalfEven(t.gross * f, 2),
  }
}

/**
 * Invoice total in the invoice currency.
 * Half-to-even so repeated sums never drift.
 */
// TODO: move currency handling to money.ts
export function invoiceTotal(lines: Line[]) {
  const sum = lines.reduce((a, l) => a + l.qty * l.price, 0)
  if (sum === 0) return 0

  return roundHalfEven(sum, 2)
  // banker's rounding avoids 0.005 drift
}
`,
  'src/invoice/round.ts': `export function roundHalfEven(n: number, dp: number) {
  const f = 10 ** dp
  const x = n * f
  const r = Math.round(x)
  const tie = Math.abs(x % 1) === 0.5
  return (tie && r % 2 !== 0 ? r - 1 : r) / f
}
`,
  'test/totals.spec.ts': `import { invoiceTotal } from '../src/invoice/totals'
import { roundHalfEven } from '../src/invoice/round'

const lines = [
  { qty: 3, price: 3.335 },
  { qty: 1, price: 0 },
]

// Regression: flaky on CI because of float drift

describe('invoiceTotal', () => {
  it('sums lines', () => {
    expect(invoiceTotal(lines)).toBe(10.0)
  })
  it('rounds half to even', () => {
    expect(roundHalfEven(2.675, 2)).toBe(2.68)
  })
})
`,
  'package.json': `{
  "name": "billing-api",
  "version": "2.14.0",
  "scripts": {
    "test": "vitest run",
    "build": "tsc -p ."
  }
}
`,
  'README.md': '# billing-api\n\nInvoice totals, tax and discounts.\n',
};
/** How a changed file looked at `HEAD`: `[text now, text at HEAD]`. */
const HEAD_EDITS: Record<string, [string, string]> = {
  'src/invoice/totals.ts': ["  return roundHalfEven(sum, 2)\n  // banker's rounding avoids 0.005 drift", '  return Math.round(sum * 100) / 100'],
  'test/totals.spec.ts': ['toBe(10.0)', 'toBe(10.01)'],
};
const MAX_FILE_BYTES = 2 * 1024 * 1024;

/** Stand-in for SHA-256: 64 hex digits that change whenever the text does. */
const mockSha = (text: string): string => {
  let out = '';
  for (let seed = 0; seed < 8; seed++) {
    let h = (0x811c9dc5 ^ Math.imul(seed, 0x9e3779b1)) >>> 0;
    for (let i = 0; i < text.length; i++) h = Math.imul(h ^ text.charCodeAt(i), 0x01000193) >>> 0;
    out += h.toString(16).padStart(8, '0');
  }
  return out;
};

/** MCP servers per agent (`spec/mock.html` MCP); `live` is what a live check finds. */
const MOCK_CONNECTORS: Record<AgentKind, { configPath: string; servers: { name: string; transport: string; live: ConnectorStatus }[] }> = {
  terminal: { configPath: '', servers: [] },
  claude_code: {
    configPath: '~/.claude.json',
    servers: [
      { name: 'Agent browser', transport: 'stdio', live: { state: 'connected', tools: 14 } },
      { name: 'Jfrog', transport: 'http', live: { state: 'connected', tools: 22 } },
      { name: 'Mcp atlassian', transport: 'stdio', live: { state: 'connected', tools: 61 } },
      { name: 'Playwright', transport: 'stdio', live: { state: 'connected', tools: 21 } },
      { name: 'Mermaid Chart', transport: 'http', live: { state: 'needs_auth' } },
    ],
  },
  codex: {
    configPath: '~/.codex/config.toml',
    servers: [
      { name: 'Playwright', transport: 'stdio', live: { state: 'connected', tools: 21 } },
      { name: 'GitHub', transport: 'http', live: { state: 'failed', reason: 'authentication failed' } },
    ],
  },
  opencode: {
    configPath: 'opencode.json',
    servers: [{ name: 'Context7', transport: 'http', live: { state: 'connected', tools: 2 } }],
  },
};
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
  private workspaces: Workspace[];
  private projects: Project[];
  private sessions: Session[];
  private settings: Settings;
  private usage: UsageToday;
  private readonly daemon: Snapshot['daemon'];
  private readonly listeners = new Set<(e: DaemonEvent) => void>();
  private readonly connListeners = new Set<(s: ConnectionState) => void>();
  private readonly pty = new Map<SessionId, Set<(d: Uint8Array) => void>>();
  private readonly screens = new Map<SessionId, string>();
  /** Files written through `writeFile`, by `<session>:<path>`; everything else reads from `MOCK_SOURCES`. */
  private readonly written = new Map<string, string>();
  private readonly tasks = new Map<SessionId, BackgroundTask[]>();
  private readonly steps: ScriptStep[];
  private nextStep = 0;
  private elapsed = 0;
  private nextPrompt = 1;
  private nextSession = 8;
  private nextProject = 6;
  private nextWorkspace = 2;
  private timer: ReturnType<typeof setInterval> | null = null;

  constructor(opts: MockClientOptions = {}) {
    this.now = opts.now ?? Date.now;
    const delta = this.now() - FIXTURE_NOW;
    const snap = clone(FIXTURE);
    this.workspaces = snap.workspaces;
    this.projects = snap.projects;
    this.settings = snap.settings;
    this.usage = snap.usage;
    this.daemon = { ...snap.daemon, started_at: snap.daemon.started_at + delta };
    this.sessions = snap.sessions.map((s) => ({
      ...s,
      created_at: s.created_at + delta,
      last_activity: s.last_activity + delta,
      running_since: s.running_since === null ? null : s.running_since + delta,
    }));
    for (const s of this.sessions) {
      if (s.perm && !s.perm.id) s.perm.id = this.newPromptId();
      if (s.agent === 'claude_code') {
        this.tasks.set(s.id, [{ pid: 4711, command: 'npm run dev', started_at: this.now() - 12 * 60_000 }]);
      }
    }
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
    s.perm = status === 'perm' && perm ? { ...perm, id: perm.id ?? this.newPromptId() } : null;
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
    return clone({
      daemon: this.daemon,
      workspaces: this.workspaces,
      projects: this.projects,
      sessions: this.sessions,
      settings: this.settings,
      usage: this.usage,
    });
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

  async moveProject(projectId: ProjectId, workspaceId: WorkspaceId): Promise<Project> {
    const p = this.mustProject(projectId);
    this.mustWorkspace(workspaceId);
    if (p.workspace_id !== workspaceId) {
      p.workspace_id = workspaceId;
      this.emit({ event: 'project_updated', data: clone(p) });
    }
    return clone(p);
  }

  async createWorkspace(name: string): Promise<Workspace> {
    const clean = name.trim();
    if (clean === '' || clean.length > 100) {
      throw new DaemonError({ code: 'invalid_request', message: 'workspace name must be 1-100 characters' });
    }
    this.nameFree(clean);
    const ws: Workspace = { id: `w${this.nextWorkspace++}`, name: clean };
    this.workspaces.push(ws);
    this.emit({ event: 'workspace_added', data: clone(ws) });
    return clone(ws);
  }

  async renameWorkspace(workspaceId: WorkspaceId, name: string): Promise<Workspace> {
    const ws = this.mustWorkspace(workspaceId);
    const clean = name.trim();
    if (clean === '' || clean.length > 100) {
      throw new DaemonError({ code: 'invalid_request', message: 'workspace name must be 1-100 characters' });
    }
    this.nameFree(clean, workspaceId);
    ws.name = clean;
    this.emit({ event: 'workspace_updated', data: clone(ws) });
    return clone(ws);
  }

  async deleteWorkspace(workspaceId: WorkspaceId, moveProjects: boolean): Promise<void> {
    const ws = this.mustWorkspace(workspaceId);
    const home = this.workspaces[0];
    if (!home || ws.id === home.id) {
      throw new DaemonError({ code: 'conflict', message: 'the default workspace cannot be deleted' });
    }
    const inside = this.projects.filter((p) => p.workspace_id === ws.id);
    if (inside.length > 0 && !moveProjects) {
      throw new DaemonError({ code: 'conflict', message: `the workspace still has ${inside.length} project(s)` });
    }
    for (const p of inside) {
      p.workspace_id = home.id;
      this.emit({ event: 'project_updated', data: clone(p) });
    }
    this.workspaces = this.workspaces.filter((w) => w.id !== ws.id);
    this.emit({ event: 'workspace_removed', data: { workspace_id: ws.id } });
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
          behind: 0,
          added: 0,
          removed: 0,
          pr: null,
        }
      : { ...NO_GIT };
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

  async resolvePermission(sessionId: SessionId, decision: PermDecision, opts: { via?: PermVia; promptId?: string } = {}): Promise<void> {
    const s = this.must(sessionId);
    if (opts.promptId !== undefined && s.perm?.id !== opts.promptId) {
      throw new DaemonError({ code: 'conflict', message: 'that prompt is no longer open' });
    }
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
    if (!p.is_git || !s) return { ...NO_GIT, is_git: p.is_git, branch: p.is_git ? 'main' : '' };
    return clone(s.git);
  }

  async getDiff(sessionId: SessionId, base: DiffBase = 'head'): Promise<GitDiff> {
    const s = this.must(sessionId);
    if (!s.git.is_git) return { is_git: false, files: [], truncated: false };
    // the merge-base view also holds what the branch committed
    const changed = s.git.dirty > 0 || (base === 'merge_base' && s.git.ahead > 0);
    return clone(changed ? MOCK_DIFF : { is_git: true, files: [], truncated: false });
  }

  async getTranscript(sessionId: SessionId, limit?: number): Promise<Transcript> {
    const s = this.must(sessionId);
    const turns = this.turnsOf(s);
    const last = limit === undefined ? turns : turns.slice(-limit);
    return { available: true, turns: clone(last), truncated: last.length < turns.length, source: s.agent === 'codex' ? 'codex' : 'claude' };
  }

  async getReview(sessionId: SessionId): Promise<Review> {
    const s = this.must(sessionId);
    if (!s.git.is_git) return { base: 'HEAD', ahead: 0, behind: 0, commits: [], truncated: false };
    const at = this.now();
    const commits = Array.from({ length: s.git.ahead }, (_, i) => {
      const known = MOCK_COMMITS[i];
      return {
        sha: known?.sha ?? (0xb10c000 + i).toString(16),
        subject: known?.subject ?? `Work in progress (${i + 1})`,
        author: LABEL[s.agent],
        at: at - (known?.minutesAgo ?? 11 + 7 * (i - 1)) * 60_000,
      };
    });
    return {
      base: s.git.worktree || s.git.ahead > 0 ? 'main' : 'HEAD',
      ahead: s.git.ahead,
      behind: s.git.behind ?? 0,
      commits,
      truncated: false,
    };
  }

  async listFiles(sessionId: SessionId, changedOnly = false): Promise<FileTree> {
    const s = this.must(sessionId);
    const changes = this.changesOf(s);
    const entries: FileEntry[] = MOCK_TREE.map((raw) => {
      const dir = raw.endsWith('/');
      const path = dir ? raw.slice(0, -1) : raw;
      const c = changes[path];
      return { path, dir, status: c?.status ?? null, added: c?.added ?? 0, removed: c?.removed ?? 0 };
    });
    return {
      root: s.git.worktree_path ?? this.mustProject(s.project_id).path,
      is_git: s.git.is_git,
      entries: changedOnly ? entries.filter((e) => e.status !== null) : entries,
      truncated: false,
    };
  }

  async readFile(sessionId: SessionId, path: string, rev?: string): Promise<FileContent> {
    const s = this.must(sessionId);
    this.mustFile(path);
    let text = this.written.get(`${s.id}:${path}`) ?? MOCK_SOURCES[path] ?? `// ${path}\n\nexport {}\n`;
    if (rev !== undefined) {
      const change = this.changesOf(s)[path];
      if (change?.status === 'added') throw new DaemonError({ code: 'not_found', message: `${path} does not exist at ${rev}` });
      const edit = HEAD_EDITS[path];
      if (change && edit) text = text.replace(edit[0], edit[1]);
    }
    return this.fileOf(path, text);
  }

  async writeFile(sessionId: SessionId, path: string, text: string, expectedSha?: string): Promise<FileContent> {
    const current = await this.readFile(sessionId, path);
    if (expectedSha !== undefined && expectedSha !== current.sha) {
      throw new DaemonError({ code: 'conflict', message: `${path} changed since it was read` });
    }
    if (enc.encode(text).length > MAX_FILE_BYTES) {
      throw new DaemonError({ code: 'invalid_request', message: 'files over 2 MiB cannot be edited' });
    }
    this.written.set(`${sessionId}:${path}`, text);
    return this.fileOf(path, text);
  }

  async getUsage(sessionId: SessionId): Promise<Usage> {
    this.must(sessionId);
    const start = this.now() - 41 * 60_000;
    const span = (status: Status, from: number, to: number) => ({ status, from: start + from * 60_000, to: start + to * 60_000 });
    return {
      started_at: start,
      total_ms: 41 * 60_000,
      working_ms: 37 * 60_000,
      waiting_ms: 4 * 60_000,
      lanes: [
        { name: 'lead', kind: 'lead', spans: [span('run', 0, 3), span('perm', 3, 7), span('run', 7, 41)], cost_micros: 820_000 },
        { name: 'explore-repo', kind: 'sub_agent', spans: [span('done', 0.5, 9)], cost_micros: 190_000 },
        { name: 'test-runner', kind: 'sub_agent', spans: [span('run', 23, 41)], cost_micros: 130_000 },
      ],
      tokens: { input: 61_000, cache_read: 26_000, cache_write: 0, output: 9_000 },
      spend: { session: 1_140_000, project: 2_310_000, all: 4_180_000 },
      model: 'Opus 5.5',
      priced: true,
    };
  }

  async exportTranscript(sessionId: SessionId, format: ExportFormat): Promise<Export> {
    const s = this.must(sessionId);
    const turns = this.turnsOf(s);
    const who = (t: TranscriptTurn) => (t.role === 'user' ? 'You' : LABEL[s.agent]);
    const tool = (t: TranscriptTurn) => `${t.tool?.name ?? ''} ${t.tool?.summary ?? ''}${t.tool?.result ? ` → ${t.tool.result}` : ''}`;
    let content: string;
    let ext: string;
    if (format === 'json_lines') {
      ext = 'jsonl';
      content = turns.map((t) => JSON.stringify(t)).join('\n') + '\n';
    } else if (format === 'markdown') {
      ext = 'md';
      const body = turns.map((t) => (t.role === 'tool' ? `- \`${tool(t)}\`` : `**${who(t)}:** ${t.text}`));
      content = `# ${s.title}\n\n${body.join('\n\n')}\n`;
    } else {
      ext = 'txt';
      content = turns.map((t) => (t.role === 'tool' ? `[${tool(t)}]` : `${who(t)}: ${t.text}`)).join('\n') + '\n';
    }
    const slug = s.title.toLowerCase().replace(/[^a-z0-9]+/g, '-').replace(/^-|-$/g, '') || 'transcript';
    return { file_name: `${slug}.${ext}`, content, redactions: 0 };
  }

  async getSessionInfo(sessionId: SessionId): Promise<SessionInfo> {
    const s = this.must(sessionId);
    return {
      agent_version: s.agent === 'claude_code' ? '2.1.309' : null,
      model: s.agent === 'terminal' ? null : 'Opus 5.5',
      machine: "Naim's MacBook Pro",
      cwd: this.mustProject(s.project_id).path,
      worktree: s.git.worktree_path,
      source: s.source,
      tasks: clone(this.tasks.get(s.id) ?? []),
    };
  }

  async stopTask(sessionId: SessionId, pid: number): Promise<void> {
    const s = this.must(sessionId);
    const tasks = this.tasks.get(s.id) ?? [];
    if (!tasks.some((t) => t.pid === pid)) throw new DaemonError({ code: 'not_found', message: `no background task ${pid}` });
    this.tasks.set(s.id, tasks.filter((t) => t.pid !== pid));
  }

  async listConnectors(sessionId: SessionId): Promise<Connector[]> {
    return this.connectorsOf(this.must(sessionId), false);
  }

  async checkConnectors(sessionId: SessionId): Promise<Connector[]> {
    // OpenCode has no live check: its servers stay `configured`
    const s = this.must(sessionId);
    return this.connectorsOf(s, this.settings.external.mcp_live && s.agent !== 'opencode');
  }

  /** Change today's tokens and cost like a daemon refresh would (tests, demos). */
  setUsage(usage: UsageToday): void {
    this.usage = clone(usage);
    this.emit({ event: 'usage_changed', data: clone(usage) });
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

  private mustWorkspace(id: WorkspaceId): Workspace {
    const w = this.workspaces.find((x) => x.id === id);
    if (!w) throw new DaemonError({ code: 'not_found', message: `no workspace ${id}` });
    return w;
  }

  /** Names are unique, case-insensitive. */
  private nameFree(name: string, except?: WorkspaceId): void {
    if (this.workspaces.some((w) => w.id !== except && w.name.toLowerCase() === name.toLowerCase())) {
      throw new DaemonError({ code: 'conflict', message: 'a workspace with that name exists' });
    }
  }

  private mustProject(id: ProjectId): Project {
    const p = this.projects.find((x) => x.id === id);
    if (!p) throw new DaemonError({ code: 'not_found', message: `no project ${id}` });
    return p;
  }

  private newPromptId(): string {
    return `prompt-${this.nextPrompt++}`;
  }

  /** The agent's transcript turns; plain terminals and OpenCode have none (like `get_transcript` in proto 3). */
  private turnsOf(s: Session): TranscriptTurn[] {
    if (s.agent === 'terminal' || s.agent === 'opencode') {
      throw new DaemonError({ code: 'unavailable', message: `Not available for ${LABEL[s.agent]}` });
    }
    return MOCK_TURNS.map((t, i) => ({ ...t, at: s.created_at + i * 5_000 }));
  }

  /** Changed files of a session: the mock's uncommitted changes, only where git sees some. */
  private changesOf(s: Session): typeof MOCK_CHANGES {
    return s.git.is_git && s.git.dirty > 0 ? MOCK_CHANGES : {};
  }

  /** Paths are relative to the session folder and stay inside it; only files of the mock tree exist. */
  private mustFile(path: string): void {
    if (path === '' || path.startsWith('/') || path.split('/').includes('..')) {
      throw new DaemonError({ code: 'invalid_request', message: `${path} is outside the session folder` });
    }
    if (MOCK_TREE.includes(`${path}/`)) throw new DaemonError({ code: 'invalid_request', message: `${path} is a folder` });
    if (!MOCK_TREE.includes(path)) throw new DaemonError({ code: 'not_found', message: `no file ${path}` });
  }

  private fileOf(path: string, text: string): FileContent {
    return { path, text, sha: mockSha(text), binary: false, size: enc.encode(text).length };
  }

  private connectorsOf(s: Session, live: boolean): Connector[] {
    const { configPath, servers } = MOCK_CONNECTORS[s.agent];
    return servers.map((x) => ({
      agent: s.agent,
      name: x.name,
      transport: x.transport,
      config_path: configPath,
      status: live ? x.live : { state: 'configured' },
    }));
  }

  /** Daemon-side notification decision: settings table, pause toggle, per-project override. */
  private notify(s: Session, kind: NotificationKind, body: string | null): void {
    const n = this.settings.notifications;
    if (n.paused) return;
    const project = this.mustProject(s.project_id);
    const mode = project.notifications;
    // banner off with sound on is sound only
    const banner = mode.mode === 'custom' ? mode[kind] : n[kind].banner;
    if (mode.mode === 'mute' || (!banner && !n[kind].sound)) return;
    this.emit({
      event: 'notification',
      data: {
        session_id: s.id,
        kind,
        title: `${project.name} · ${LABEL[s.agent]}`,
        subtitle: SUBTITLE[kind],
        body,
        banner,
        sound: n[kind].sound,
        ...(n[kind].sound_name ? { sound_name: n[kind].sound_name } : {}),
        ...(kind === 'perm' && s.perm?.id ? { prompt_id: s.perm.id } : {}),
        at: this.now(),
      },
    });
  }
}
