import type { AgentKind, Project, ProjectId, Session, SessionId, Status } from '../client/types';

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
  run: 'In progress',
  perm: 'Needs permission',
  fail: 'Failed',
  done: 'Done',
  idle: 'Idle',
  unknown: 'Unknown',
};

/** A session needs you when it, or one of its sub-agents, waits for permission or failed. */
export const needsYou = (s: Session): boolean =>
  NEED.includes(s.status) || s.sub_agents.some((x) => NEED.includes(x.status));

export const needYouCount = (sessions: readonly Session[]): number => sessions.filter(needsYou).length;

/** Highest-ranked status among the sessions (`idle` for none). Sub-agents do not change it. */
export function rollup(sessions: readonly Session[]): Status {
  let top: Status = 'idle';
  for (const s of sessions) if (RANK[s.status] > RANK[top]) top = s.status;
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

/** Whole-list signature of everything the clock changes in the sidebar (see `useCoarseNow`). */
export const timeSignature = (sessions: readonly Session[], now: number): string =>
  sessions.map((s) => sessionAge(s, now)).join('|');
