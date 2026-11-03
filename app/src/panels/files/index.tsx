import type { SessionId } from '../../lib/client/types';

// Placeholder: wp13 owns this folder. Keep the `FilesPanel` export and its props: `RightPanel` mounts it.
export function FilesPanel({ sessionId }: { sessionId: SessionId | null }) {
  return (
    <div data-testid="panel-files" data-session-id={sessionId ?? undefined} className="text-[13px] text-dim">
      Files
    </div>
  );
}
