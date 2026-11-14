import { useEffect, useState } from 'react';
import { useClient } from '../lib/client';
import type { Snapshot } from '../lib/client/types';

/**
 * The daemon snapshot while `enabled` (an overlay is open). Overlays are short-lived, so they refetch
 * on any event and on reconnect instead of keeping a store of their own.
 */
export function useSnapshot(enabled = true): Snapshot | null {
  const client = useClient();
  const [snap, setSnap] = useState<Snapshot | null>(null);
  useEffect(() => {
    if (!enabled) return;
    let alive = true;
    const load = () =>
      client
        .getSnapshot()
        .then((s) => alive && setSnap(s))
        .catch(() => {});
    void load();
    const offEvent = client.onEvent(() => void load());
    const offConn = client.onConnection((s) => s === 'reconnected' && void load());
    return () => {
      alive = false;
      offEvent();
      offConn();
    };
  }, [client, enabled]);
  return snap;
}
