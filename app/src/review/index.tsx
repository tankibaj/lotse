import type { SessionId } from '../lib/client/types';

// Placeholder: wp17 owns this folder. Keep the `Review` export and its props: the shell mounts it in the `review` slot
// (the mock's `#diff`) and closes it on Esc.
export function Review({ sessionId, file, onClose }: { sessionId: SessionId; file?: string; onClose: () => void }) {
  return (
    <div className="absolute inset-0 z-30 flex flex-col bg-win text-txt" role="dialog" aria-modal="true" aria-label="Review">
      <div className="flex h-11 shrink-0 items-center gap-3 border-b px-3">
        <button type="button" className="rounded-md px-2 py-1 text-[13px] hover:bg-sel" onClick={onClose}>
          ‹ Back <span className="text-[11px] text-dim">esc</span>
        </button>
        <span className="truncate text-[13px] font-medium">
          {sessionId}
          {file ? ` · ${file}` : ''}
        </span>
      </div>
    </div>
  );
}
