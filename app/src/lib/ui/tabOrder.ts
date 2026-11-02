import { useMemo, useSyncExternalStore } from 'react';
import type { ProjectId, Session, SessionId } from '../client/types';

// Tab order is UI state, not daemon state: `lotse-tab-order` in localStorage maps a project id to its session ids
// in the order the user dragged them to. Sessions not in the list (new ones) follow in the daemon's order.

const KEY = 'lotse-tab-order';

type Saved = Record<ProjectId, SessionId[]>;

const listeners = new Set<() => void>();

const readRaw = (): string => {
  try {
    return localStorage.getItem(KEY) ?? '';
  } catch {
    return '';
  }
};

function parse(raw: string): Saved {
  try {
    const v: unknown = JSON.parse(raw || '{}');
    if (typeof v !== 'object' || v === null || Array.isArray(v)) return {};
    const out: Saved = {};
    for (const [project, ids] of Object.entries(v)) {
      if (Array.isArray(ids)) out[project] = ids.filter((id): id is string => typeof id === 'string');
    }
    return out;
  } catch {
    return {};
  }
}

/** `tabs` (daemon order) arranged by the saved order; unknown sessions keep their place at the end. */
export function orderTabs(tabs: readonly Session[], saved: readonly SessionId[] | undefined): Session[] {
  if (!saved?.length) return [...tabs];
  const rank = new Map(saved.map((id, i) => [id, i]));
  const known = tabs.filter((s) => rank.has(s.id)).sort((a, b) => (rank.get(a.id) ?? 0) - (rank.get(b.id) ?? 0));
  return [...known, ...tabs.filter((s) => !rank.has(s.id))];
}

/** `moving` placed right before `before` (`null`: at the end). Pure. */
export function moveTab(ids: readonly SessionId[], moving: SessionId, before: SessionId | null): SessionId[] {
  const rest = ids.filter((id) => id !== moving);
  const at = before === null ? rest.length : rest.indexOf(before);
  rest.splice(at < 0 ? rest.length : at, 0, moving);
  return rest;
}

export function saveTabOrder(projectId: ProjectId, ids: readonly SessionId[]): void {
  const saved = parse(readRaw());
  saved[projectId] = [...ids];
  try {
    localStorage.setItem(KEY, JSON.stringify(saved));
  } catch {
    // storage unavailable: the order lasts until the next reload
  }
  for (const l of [...listeners]) l();
}

/** The sessions of one project as tabs, in the user's order. Also what ⌘1…9 count. */
export const projectTabs = (sessions: readonly Session[], projectId: ProjectId | undefined): Session[] =>
  orderTabs(
    sessions.filter((s) => s.project_id === projectId),
    projectId === undefined ? undefined : parse(readRaw())[projectId],
  );

const subscribe = (l: () => void) => {
  listeners.add(l);
  return () => listeners.delete(l);
};

/** React hook: re-renders when the order is saved. */
export function useProjectTabs(sessions: readonly Session[], projectId: ProjectId | undefined): Session[] {
  const raw = useSyncExternalStore(subscribe, readRaw);
  return useMemo(
    () =>
      orderTabs(
        sessions.filter((s) => s.project_id === projectId),
        projectId === undefined ? undefined : parse(raw)[projectId],
      ),
    [sessions, projectId, raw],
  );
}
