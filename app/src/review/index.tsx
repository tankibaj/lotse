import { useRef } from 'react';
import type { SessionId } from '../lib/client/types';
import { useSessions } from '../panes/useSessions';
import { useDialogFocus } from '../sheets/ui';

// Placeholder: wp17 owns this folder. Keep the `Review` export and its props: the shell mounts it in the `review` slot
// (the mock's `#diff`) and closes it on Esc (in the capture phase, so a focused terminal cannot swallow it).
// Focus moves into the dialog on open and goes back to where it was on close.
export function Review({ sessionId, file, onClose }: { sessionId: SessionId; file?: string; onClose: () => void }) {
  const box = useRef<HTMLDivElement>(null);
  const trap = useDialogFocus(box);
  const { sessions } = useSessions();
  const title = sessions.find((s) => s.id === sessionId)?.title || 'Session';
  return (
    <div ref={box} onKeyDown={trap} className="absolute inset-0 z-30 flex flex-col bg-win text-txt" role="dialog" aria-modal="true" aria-label="Review">
      <div className="flex h-11 shrink-0 items-center gap-3 border-b px-3">
        <button type="button" data-autofocus className="rounded-md px-2 py-1 text-[13px] hover:bg-sel" onClick={onClose}>
          ‹ Back <span className="text-[11px] text-dim">esc</span>
        </button>
        <span className="truncate text-[13px] font-medium">
          {title}
          {file ? ` · ${file}` : ''}
        </span>
      </div>
    </div>
  );
}
