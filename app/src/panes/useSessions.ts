import { useEffect, useState } from 'react';
import { useClient } from '../lib/client';
import type { Project, Session } from '../lib/client';

export interface SessionsState {
  sessions: Session[];
  projects: Project[];
  /** `false` until the first snapshot arrived. */
  loaded: boolean;
}

/**
 * Sessions and projects for tabs and panes: snapshot first, then events; refetched on `reconnected`
 * (events were missed). Status only moves through `status_changed`, so tabs stay current.
 */
export function useSessions(): SessionsState {
  const client = useClient();
  const [state, setState] = useState<SessionsState>({ sessions: [], projects: [], loaded: false });

  useEffect(() => {
    let alive = true;
    const load = () =>
      client.getSnapshot().then((s) => {
        if (alive) setState({ sessions: s.sessions, projects: s.projects, loaded: true });
      });
    void load();

    const patch = (id: string, f: (s: Session) => Session) =>
      setState((st) => ({ ...st, sessions: st.sessions.map((s) => (s.id === id ? f(s) : s)) }));

    const offEvent = client.onEvent((e) => {
      switch (e.event) {
        case 'session_created':
          setState((st) => ({ ...st, sessions: [...st.sessions.filter((s) => s.id !== e.data.id), e.data] }));
          break;
        case 'session_updated':
          patch(e.data.id, () => e.data);
          break;
        case 'session_closed':
          setState((st) => ({ ...st, sessions: st.sessions.filter((s) => s.id !== e.data.session_id) }));
          break;
        case 'status_changed':
          patch(e.data.session_id, (s) => ({ ...s, status: e.data.status, perm: e.data.perm, running_since: e.data.running_since }));
          break;
        case 'project_added':
          setState((st) => ({ ...st, projects: [...st.projects.filter((p) => p.id !== e.data.id), e.data] }));
          break;
        case 'project_updated':
          setState((st) => ({ ...st, projects: st.projects.map((p) => (p.id === e.data.id ? e.data : p)) }));
          break;
        case 'project_removed':
          setState((st) => ({ ...st, projects: st.projects.filter((p) => p.id !== e.data.project_id) }));
          break;
        default:
          break;
      }
    });
    const offConn = client.onConnection((c) => {
      if (c === 'reconnected') void load();
    });
    return () => {
      alive = false;
      offEvent();
      offConn();
    };
  }, [client]);

  return state;
}
