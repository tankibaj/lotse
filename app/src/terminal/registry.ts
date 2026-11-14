import type { SessionId } from '../lib/client';

// The terminals on screen, by session, for the few things outside a pane that must know a terminal's state
// (the composer pastes the way `Terminal.paste` does, which depends on the pane's bracketed-paste mode).

/** The part of an xterm `Terminal` that others may read. */
export interface TerminalProbe {
  readonly modes: { readonly bracketedPasteMode: boolean };
}

const mounted = new Map<SessionId, TerminalProbe>();

/** A pane's terminal came up for `sessionId`. Returns the cleanup that forgets it again. */
export function registerTerminal(sessionId: SessionId, terminal: TerminalProbe): () => void {
  mounted.set(sessionId, terminal);
  return () => {
    if (mounted.get(sessionId) === terminal) mounted.delete(sessionId);
  };
}

/** Does the session's terminal have bracketed-paste mode on (`CSI ? 2004 h`)? `false` when none is on screen. */
export function bracketedPasteOn(sessionId: SessionId): boolean {
  return mounted.get(sessionId)?.modes.bracketedPasteMode ?? false;
}
