import { useEffect, useState } from 'react';
import { useClient } from '../../lib/client';
import type { DaemonEvent, SessionId, Usage } from '../../lib/client/types';

export type UsageState =
  | { kind: 'none' }
  | { kind: 'loading' }
  | { kind: 'ready'; usage: Usage }
  | { kind: 'error'; message: string };

/** Events after which the time split, lanes or spend of `sessionId` may have changed. `usage_changed` is global. */
function touches(e: DaemonEvent, sessionId: SessionId): boolean {
  switch (e.event) {
    case 'usage_changed':
      return true;
    case 'session_updated':
      return e.data.id === sessionId;
    case 'status_changed':
    case 'sub_agents_changed':
    case 'sub_agent_status':
    case 'session_exited':
      return e.data.session_id === sessionId;
    default:
      return false;
  }
}

/**
 * `getUsage` of one session, refetched on `usage_changed`, the session's status events and a reconnect.
 * A refetch asked for while one is in flight runs once after it, so a burst of events costs two requests at most.
 */
export function useUsage(sessionId: SessionId | null): UsageState {
  const client = useClient();
  // keyed by session so a render right after a switch never shows the previous session's numbers
  const [keyed, setKeyed] = useState<{ id: SessionId; state: UsageState } | null>(null);

  useEffect(() => {
    if (sessionId === null) return;
    const setState = (next: UsageState | ((s: UsageState) => UsageState)) =>
      setKeyed((k) => {
        const prev: UsageState = k?.id === sessionId ? k.state : { kind: 'loading' };
        return { id: sessionId, state: typeof next === 'function' ? next(prev) : next };
      });
    let alive = true;
    let inflight = false;
    let again = false;
    const load = async () => {
      if (inflight) {
        again = true;
        return;
      }
      inflight = true;
      try {
        const usage = await client.getUsage(sessionId);
        if (alive) setState({ kind: 'ready', usage });
      } catch (e) {
        // keep the last good numbers when only a refresh fails
        if (alive) setState((s) => (s.kind === 'ready' ? s : { kind: 'error', message: e instanceof Error ? e.message : String(e) }));
      } finally {
        inflight = false;
        if (alive && again) {
          again = false;
          void load();
        }
      }
    };
    setState({ kind: 'loading' });
    void load();
    const stops = [
      client.onEvent((e) => {
        if (touches(e, sessionId)) void load();
      }),
      client.onConnection((c) => {
        if (c === 'reconnected') void load();
      }),
    ];
    return () => {
      alive = false;
      stops.forEach((stop) => stop());
    };
  }, [client, sessionId]);

  if (sessionId === null) return { kind: 'none' };
  return keyed?.id === sessionId ? keyed.state : { kind: 'loading' };
}
