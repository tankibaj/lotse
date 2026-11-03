import type { SessionId } from '../../lib/client/types';

// Placeholder: wp16 owns this folder. Keep the `SessionPanel` export and its props: `RightPanel` mounts it.
export function SessionPanel({ sessionId }: { sessionId: SessionId | null }) {
  return (
    <div data-testid="panel-session" data-session-id={sessionId ?? undefined} className="text-[13px] text-dim">
      Session
    </div>
  );
}
