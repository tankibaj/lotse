import type { SessionId } from '../../lib/client/types';

// Placeholder: wp11 owns this folder. Keep the `ChangesPanel` export and its props: `RightPanel` mounts it.
export function ChangesPanel({ sessionId }: { sessionId: SessionId | null }) {
  return (
    <div data-testid="panel-changes" data-session-id={sessionId ?? undefined} className="text-[13px] text-dim">
      Changes
    </div>
  );
}
