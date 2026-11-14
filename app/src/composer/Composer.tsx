import { useCallback, useEffect, useRef, useState } from 'react';
import type { KeyboardEvent as ReactKeyboardEvent } from 'react';
import { useClient } from '../lib/client';
import type { SessionId } from '../lib/client/types';
import { onUiCommand } from '../lib/ui/commands';
import { getSelectedSessionId, useSelectedSessionId } from '../lib/ui/selection';
import { useSessions } from '../panes/useSessions';
import { NotSubmitted, sendPrompt } from './send';

/**
 * The prompt composer: a multi-line box under the panes, for the focused (selected) session.
 * Return sends the text as a paste (`send.ts`), then a carriage return of its own (with the Claude mod live: one write the
 * daemon hands to the mod); ⇧Return adds a line, Esc closes. One draft per
 * session survives closing; a sent draft is gone. A plain `<textarea>`, so macOS Dictation and input methods work.
 * Opened by the `openComposer` command (⌘⇧Return in the shell); focus returns to where it was when it closes.
 */
export function Composer() {
  const client = useClient();
  const sessionId = useSelectedSessionId();
  const { sessions } = useSessions();
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
  // With more than one pane the box says which session it sends to (the panes mark theirs with `data-pane`; a pane
  // opened or closed meanwhile changes the selection, which renders this again).
  const paneCount = document.querySelectorAll('[data-pane]').length;
  const title = sessions.find((s) => s.id === sessionId)?.title || 'this session';
  const setText = (id: SessionId, value: string) => setDrafts((d) => ({ ...d, [id]: value }));

  const send = () => {
    if (!text.trim()) return;
    const id = sessionId;
    setText(id, '');
    close();
    // with the Claude mod live the daemon submits through it (and types it when the mod cannot)
    const viaMod = sessions.find((s) => s.id === id)?.source_layer === 'mod';
    sendPrompt(client, id, text, viaMod).catch((e: unknown) => {
      const message = e instanceof Error ? e.message : 'Could not send';
      if (e instanceof NotSubmitted) {
        // the words are already in the agent's input: no draft to restore, just say Return did not follow
        setError(`pasted, but Return did not follow: ${message}`);
      } else {
        // keep the words: put the draft back and show why it did not go
        setText(id, text);
        setError(`Not sent: ${message}`);
      }
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
    // In the flow under the panes (not over them): the terminals shrink by its height while it is open, refit through
    // their ResizeObserver, and grow back when it closes. The focus ring is the card's, not the textarea's own halo.
    <div
      className="mx-3 mb-3 shrink-0 rounded-xl border bg-win p-2 text-txt shadow-lg focus-within:outline-3 focus-within:outline-blue/40"
      data-testid="composer"
    >
      <textarea
        ref={box}
        rows={3}
        value={text}
        onChange={(e) => setText(sessionId, e.target.value)}
        onKeyDown={onKeyDown}
        aria-label="Prompt composer"
        placeholder="Message the agent"
        className="block max-h-48 w-full resize-none bg-transparent px-1 text-[13px] text-txt outline-none placeholder:text-dim focus-visible:outline-none!"
      />
      <div className="mt-1 flex items-center gap-2 px-1 text-[11px] text-dim">
        {paneCount > 1 && <span className="min-w-0 truncate">Send to {title}</span>}
        {error && (
          <span role="alert" className="text-red">
            {error}
          </span>
        )}
        <span className="ml-auto shrink-0">↩ send · ⇧↩ new line · esc close</span>
      </div>
    </div>
  );
}
