import type { DaemonClient, SessionId } from '../lib/client';
import { bracketedPasteOn } from '../terminal/registry';

/** Pause between the pasted text and the Return that submits it (the daemon's first prompt does the same). */
export const SUBMIT_DELAY_MS = 150;

/**
 * `text` as a paste, byte for byte what xterm.js writes for `Terminal.paste`: every line break becomes CR, and with
 * bracketed-paste mode on the whole thing sits between ESC[200~ and ESC[201~, so an agent TUI takes it as one paste
 * instead of submitting at the first line break.
 */
export function pasteBytes(text: string, bracketed: boolean): string {
  const body = text.replace(/\r?\n/g, '\r');
  return bracketed ? `\x1b[200~${body}\x1b[201~` : body;
}

/** The text went out but the Return after it did not: the words are in the agent's input box, unsubmitted. */
export class NotSubmitted extends Error {}

/**
 * Send a prompt to a session: the text as a paste (bracketed when the pane's terminal asked for it), then, as its own
 * write after `SUBMIT_DELAY_MS`, the CR that submits it. Trailing line breaks are dropped so only one submit goes out.
 */
export async function sendPrompt(client: Pick<DaemonClient, 'sendInput'>, sessionId: SessionId, text: string): Promise<void> {
  await client.sendInput(sessionId, pasteBytes(text.replace(/[\r\n]+$/, ''), bracketedPasteOn(sessionId)));
  await new Promise((resolve) => setTimeout(resolve, SUBMIT_DELAY_MS));
  try {
    await client.sendInput(sessionId, '\r');
  } catch (e) {
    throw new NotSubmitted(e instanceof Error ? e.message : 'Could not send');
  }
}
