import type { AgentKind, Project, ProjectId, Session, SessionId, Status, UsageToday, Workspace } from '../client/types';

// Pure selectors over the snapshot: roll-up, need-you, coarse time, visible order. No React, no clock reads.

/** Roll-up order for collapsed projects: perm > fail > run > done > idle. `unknown` ranks like idle. */
export const RANK: Record<Status, number> = { perm: 4, fail: 3, run: 2, done: 1, idle: 0, unknown: 0 };

const NEED: readonly Status[] = ['perm', 'fail'];

export const AGENT_LABEL: Record<AgentKind, string> = {
  terminal: 'Terminal',
  claude_code: 'Claude Code',
  codex: 'Codex',
  opencode: 'OpenCode',
};

export const STATUS_LABEL: Record<Status, string> = {
  run: 'Running',
  perm: 'Needs permission',
  fail: 'Failed',
  done: 'Done',
  idle: 'Idle',
  unknown: 'Detecting…',
};

/** A session needs you when it, or one of its sub-agents, waits for permission or failed. */
export const needsYou = (s: Session): boolean =>
  NEED.includes(s.status) || s.sub_agents.some((x) => NEED.includes(x.status));

export const needYouCount = (sessions: readonly Session[]): number => sessions.filter(needsYou).length;

/**
 * What the lead's icon shows. Running-lead rule: while any sub-agent runs, a lead that is not waiting or
 * failed (idle, done, unknown) shows `run`; perm and fail keep their own state.
 */
export function displayStatus(s: Session): Status {
  return RANK[s.status] < RANK.run && s.sub_agents.some((x) => x.status === 'run') ? 'run' : s.status;
}

/** Highest-ranked displayed status among the sessions (`idle` for none). */
export function rollup(sessions: readonly Session[]): Status {
  let top: Status = 'idle';
  for (const s of sessions) {
    const st = displayStatus(s);
    if (RANK[st] > RANK[top]) top = st;
  }
  return top;
}

export interface RollupSummary {
  /** Sessions with a permission request (own status or a sub-agent). */
  perm: number;
  /** Sessions that failed (own status or a sub-agent). */
  fail: number;
  top: Status;
}

/** What a collapsed project row shows: need-you counts first, else the top state. */
export function rollupSummary(sessions: readonly Session[]): RollupSummary {
  const count = (st: Status) => sessions.filter((s) => s.status === st || s.sub_agents.some((x) => x.status === st)).length;
  return { perm: count('perm'), fail: count('fail'), top: rollup(sessions) };
}

/** Coarse age for the sidebar: `<1m`, `4m`, `1h`, `1h 5m`, `2d`. Never ticks per second. */
export function coarseAge(ms: number): string {
  const min = Math.floor(Math.max(0, ms) / 60_000);
  if (min < 1) return '<1m';
  if (min < 60) return `${min}m`;
  const h = Math.floor(min / 60);
  if (h >= 48) return `${Math.floor(h / 24)}d`;
  const m = min % 60;
  return m === 0 ? `${h}h` : `${h}h ${m}m`;
}

/** Running sessions show how long they have been running, others the time since last activity. */
export function sessionAge(s: Session, now: number): string {
  const from = s.status === 'run' && s.running_since !== null ? s.running_since : s.last_activity;
  return coarseAge(now - from);
}

/** `lotse/fix-rounding` → `fix-rounding`; the full name stays in the tooltip. */
export const shortBranch = (branch: string): string => branch.replace(/^lotse\//, '');

export interface ProjectGroup {
  project: Project;
  sessions: Session[];
}

/** Projects in daemon order, each with its sessions in daemon order. */
export function groupSessions(projects: readonly Project[], sessions: readonly Session[]): ProjectGroup[] {
  return projects.map((project) => ({ project, sessions: sessions.filter((s) => s.project_id === project.id) }));
}

export interface WorkspaceGroup {
  workspace: Workspace;
  groups: ProjectGroup[];
  /** Every session of the workspace's projects (the roll-up input). */
  sessions: Session[];
}

/**
 * Workspaces in daemon order (the default one first), each with its projects in daemon order.
 * A project whose workspace is unknown (an event raced a delete) sits in the first workspace.
 */
export function groupWorkspaces(workspaces: readonly Workspace[], groups: readonly ProjectGroup[]): WorkspaceGroup[] {
  const known = new Set(workspaces.map((w) => w.id));
  return workspaces.map((workspace, i) => {
    const mine = groups.filter((g) => g.project.workspace_id === workspace.id || (i === 0 && !known.has(g.project.workspace_id)));
    return { workspace, groups: mine, sessions: mine.flatMap((g) => g.sessions) };
  });
}

/** `980`, `1.2k`, `412k`, `1.5M`: three significant digits at most. */
export function formatTokens(n: number): string {
  const trim = (x: number) => x.toFixed(1).replace(/\.0$/, '');
  if (n < 1_000) return String(n);
  if (n < 10_000) return `${trim(n / 1_000)}k`;
  if (n < 1_000_000) return `${Math.round(n / 1_000)}k`;
  return `${trim(n / 1_000_000)}M`;
}

/** Status bar: `Today 412k tokens · $4.18`; tokens only when a model has no known price; dashes before the first snapshot. */
export function formatUsage(usage: UsageToday | null): { text: string; title: string } {
  if (!usage) return { text: 'Today — tokens · —', title: 'Tokens and cost today, from the Claude Code transcripts' };
  const tokens = usage.input_tokens + usage.output_tokens + usage.cache_read_tokens + usage.cache_write_tokens;
  const detail =
    `input ${formatTokens(usage.input_tokens)} · output ${formatTokens(usage.output_tokens)} · ` +
    `cache read ${formatTokens(usage.cache_read_tokens)} · cache write ${formatTokens(usage.cache_write_tokens)}`;
  if (usage.cost_micros === null) {
    return { text: `Today ${formatTokens(tokens)} tokens`, title: `${detail}. No cost: a model without a known price was used.` };
  }
  return { text: `Today ${formatTokens(tokens)} tokens · $${(usage.cost_micros / 1_000_000).toFixed(2)}`, title: detail };
}

/** Need-you filter: only sessions that need you, empty projects dropped. Without the filter: everything. */
export function filterGroups(groups: readonly ProjectGroup[], needFilter: boolean): ProjectGroup[] {
  if (!needFilter) return [...groups];
  return groups
    .map((g) => ({ project: g.project, sessions: g.sessions.filter(needsYou) }))
    .filter((g) => g.sessions.length > 0);
}

/** Session ids in the order the sidebar shows them (collapsed projects contribute nothing). */
export function visibleSessionIds(
  groups: readonly ProjectGroup[],
  collapsed: ReadonlySet<ProjectId>,
  needFilter: boolean,
): SessionId[] {
  return groups.flatMap((g) => (needFilter || !collapsed.has(g.project.id) ? g.sessions.map((s) => s.id) : []));
}

/** ⌘J: the next session after `currentId` (wrapping) that needs you. */
export function nextNeedYou(sessions: readonly Session[], currentId: SessionId | null): SessionId | null {
  const list = sessions.filter(needsYou);
  if (list.length === 0) return null;
  const i = list.findIndex((s) => s.id === currentId);
  return list[(i + 1) % list.length]?.id ?? null;
}

/** The most urgent session that needs you (filter on: jump there first). */
export function mostUrgent(sessions: readonly Session[]): SessionId | null {
  let best: Session | null = null;
  for (const s of sessions.filter(needsYou)) if (!best || RANK[s.status] > RANK[best.status]) best = s;
  return best?.id ?? null;
}

/** An agent session still `unknown` this long after it started has no hook or status signal. */
export const NO_SIGNAL_MS = 10_000;

/** Show the "No live status yet" hint: a Claude Code / Codex / OpenCode session that is still detecting. */
export const noLiveStatus = (s: Session, now: number): boolean =>
  s.status === 'unknown' && s.agent !== 'terminal' && now - s.created_at >= NO_SIGNAL_MS;

/** Whole-list signature of everything the clock changes in the sidebar (see `useCoarseNow`). */
export const timeSignature = (sessions: readonly Session[], now: number): string =>
  sessions.map((s) => sessionAge(s, now) + (noLiveStatus(s, now) ? '!' : '')).join('|');
