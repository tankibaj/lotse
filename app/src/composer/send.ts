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
 * Starts a send meant for the Claude mod: `lotsed` hands the text after it to the mod (`$.prompt.submit`), or types it
 * as a paste and a Return itself when the mod cannot take it. Must equal `SUBMIT_MARKER` in
 * `crates/lotsed/src/two_way.rs` (a Rust test checks this file for it). Stands in for a `submit_prompt` request that
 * proto 5 does not have.
 */
export const SUBMIT_MARKER = '\u0000lotse-submit\u0000';

/** Line breaks as LF, no control characters but tab and LF (so no second marker), no trailing line breaks. */
function submitBody(text: string): string {
  const kept = [...text.replace(/\r\n?/g, '\n')].filter((c) => {
    const n = c.codePointAt(0) ?? 0;
    return n === 9 || n === 10 || (n >= 32 && n !== 127);
  });
  return kept.join('').replace(/\n+$/, '');
}

/**
 * Send a prompt to a session. With the Claude mod live (`viaMod`): one write the daemon routes to the mod. Otherwise the
 * text as a paste (bracketed when the pane's terminal asked for it), then, as its own write after `SUBMIT_DELAY_MS`, the
 * CR that submits it. Trailing line breaks are dropped so only one submit goes out.
 */
export async function sendPrompt(client: Pick<DaemonClient, 'sendInput'>, sessionId: SessionId, text: string, viaMod = false): Promise<void> {
  if (viaMod) {
    await client.sendInput(sessionId, SUBMIT_MARKER + submitBody(text));
    return;
  }
  await client.sendInput(sessionId, pasteBytes(text.replace(/[\r\n]+$/, ''), bracketedPasteOn(sessionId)));
  await new Promise((resolve) => setTimeout(resolve, SUBMIT_DELAY_MS));
  try {
    await client.sendInput(sessionId, '\r');
  } catch (e) {
    throw new NotSubmitted(e instanceof Error ? e.message : 'Could not send');
  }
}
