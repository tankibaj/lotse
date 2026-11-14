import { useMemo } from 'react';
import type { Project, Session } from '../lib/client';
import { useWorkspace } from '../lib/state';

export interface SessionsState {
  sessions: Session[];
  projects: Project[];
  /** `false` until the first snapshot arrived. */
  loaded: boolean;
}

/**
 * Sessions and projects for tabs and panes. Since WP8 this is a view of the shared workspace store
 * (`lib/state`: snapshot, events, refetch on reconnect), not a second copy of it.
 */
export function useSessions(): SessionsState {
  const { sessions, projects, loaded } = useWorkspace();
  return useMemo(() => ({ sessions, projects, loaded }), [sessions, projects, loaded]);
}
