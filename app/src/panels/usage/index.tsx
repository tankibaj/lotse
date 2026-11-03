import type { SessionId } from '../../lib/client/types';

// Placeholder: wp14 owns this folder. Keep the `UsagePanel` export and its props: `RightPanel` mounts it.
export function UsagePanel({ sessionId }: { sessionId: SessionId | null }) {
  return (
    <div data-testid="panel-usage" data-session-id={sessionId ?? undefined} className="text-[13px] text-dim">
      Usage &amp; time
    </div>
  );
}
