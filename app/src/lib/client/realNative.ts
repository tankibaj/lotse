import type { DaemonClient } from './DaemonClient';
import type { Transport } from './realClient';

// What only the native shell can do with app state: the Dock badge (need-you count) and telling the
// notifier which session is on screen (no banner for the session you are looking at).
// Loaded lazily by the real client after its first connection; `lib/state` is imported dynamically so
// the client module never depends on it statically.

export async function wireNativeShell(client: DaemonClient, t: Transport): Promise<() => void> {
  const [{ subscribeNeedYouCount }, { getSelectedSessionId, subscribeSelection }] = await Promise.all([
    import('../state'),
    import('../ui/selection'),
  ]);
  const swallow = () => {
    // the native side is optional (a failure must never break the UI)
  };
  const offBadge = subscribeNeedYouCount(client, (count) => void t.invoke('set_badge', { count }).catch(swallow));
  const report = () => void t.invoke('set_focused_session', { sessionId: getSelectedSessionId() }).catch(swallow);
  const offSelection = subscribeSelection(report);
  report();
  return () => {
    offBadge();
    offSelection();
  };
}
