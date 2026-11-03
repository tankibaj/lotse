import type { SessionId } from '../../lib/client/types';

// Placeholder: wp15 owns this folder. Keep the `TranscriptPanel` export and its props: `RightPanel` mounts it.
export function TranscriptPanel({ sessionId }: { sessionId: SessionId | null }) {
  return (
    <div data-testid="panel-transcript" data-session-id={sessionId ?? undefined} className="text-[13px] text-dim">
      Transcript
    </div>
  );
}
