import { useCallback, useEffect, useRef, useState } from 'react';
import type { KeyboardEvent as ReactKeyboardEvent } from 'react';
import { useClient } from '../lib/client';
import type { SessionId } from '../lib/client/types';
import { onUiCommand } from '../lib/ui/commands';
import { getSelectedSessionId, useSelectedSessionId } from '../lib/ui/selection';

/**
 * The prompt composer: a multi-line box over the bottom of the panes, for the focused (selected) session.
 * Return sends the text plus a carriage return as keyboard input, ⇧Return adds a line, Esc closes. One draft per
 * session survives closing; a sent draft is gone. A plain `<textarea>`, so macOS Dictation and input methods work.
 * Opened by the `openComposer` command (⌘⇧Return in the shell); focus returns to where it was when it closes.
 */
export function Composer() {
  const client = useClient();
  const sessionId = useSelectedSessionId();
  const [open, setOpen] = useState(false);
  const [drafts, setDrafts] = useState<Record<SessionId, string>>({});
  const [error, setError] = useState<string | null>(null);
  const box = useRef<HTMLTextAreaElement>(null);
  const before = useRef<HTMLElement | null>(null);

  useEffect(
    () =>
      onUiCommand('openComposer', () => {
        if (!getSelectedSessionId()) return;
        before.current ??= document.activeElement instanceof HTMLElement ? document.activeElement : null;
        setOpen(true);
        box.current?.focus();
      }),
    [],
  );
  useEffect(() => {
    if (open) box.current?.focus();
  }, [open]);

  const close = useCallback(() => {
    setOpen(false);
    setError(null);
    const back = before.current;
    before.current = null;
    back?.focus();
  }, []);

  if (!open || !sessionId) return null;
  const text = drafts[sessionId] ?? '';
  const setText = (id: SessionId, value: string) => setDrafts((d) => ({ ...d, [id]: value }));

  const send = () => {
    if (!text.trim()) return;
    const id = sessionId;
    setText(id, '');
    close();
    client.sendInput(id, `${text}\r`).catch((e: unknown) => {
      // keep the words: put the draft back and show why it did not go
      setText(id, text);
      setError(e instanceof Error ? e.message : 'Could not send');
      setOpen(true);
    });
  };

  const onKeyDown = (e: ReactKeyboardEvent<HTMLTextAreaElement>) => {
    if (e.key === 'Escape') {
      e.preventDefault();
      e.stopPropagation();
      close();
    } else if (e.key === 'Enter' && !e.shiftKey && !e.nativeEvent.isComposing) {
      e.preventDefault();
      send();
    }
  };

  return (
    <div className="absolute inset-x-3 bottom-3 z-20 rounded-xl border bg-win p-2 text-txt shadow-lg" data-testid="composer">
      <textarea
        ref={box}
        rows={3}
        value={text}
        onChange={(e) => setText(sessionId, e.target.value)}
        onKeyDown={onKeyDown}
        aria-label="Prompt composer"
        placeholder="Message the agent"
        className="block max-h-48 w-full resize-none bg-transparent px-1 text-[13px] text-txt outline-none placeholder:text-dim"
      />
      <div className="mt-1 flex items-center gap-2 px-1 text-[11px] text-dim">
        {error && (
          <span role="alert" className="text-red">
            Not sent: {error}
          </span>
        )}
        <span className="ml-auto">↩ send · ⇧↩ new line · esc close</span>
      </div>
    </div>
  );
}
