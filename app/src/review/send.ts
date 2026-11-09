import { NotSubmitted, SUBMIT_DELAY_MS, pasteBytes } from '../composer/send';
import type { DaemonClient, SessionId } from '../lib/client';
import { stripControl } from './comments';

/**
 * The review prompt as one bracketed paste, then the Return as its own write. Unlike `sendPrompt` this does not ask the
 * pane's terminal whether it wants bracketed paste: with the terminal unmounted that answer is `false` and the
 * multi-line prompt would go out as one submit per line. Control bytes are stripped again here, so nothing in the text can
 * end the paste early.
 */
export async function sendReviewPrompt(client: Pick<DaemonClient, 'sendInput'>, sessionId: SessionId, text: string): Promise<void> {
  await client.sendInput(sessionId, pasteBytes(stripControl(text).replace(/\n+$/, ''), true));
  await new Promise((resolve) => setTimeout(resolve, SUBMIT_DELAY_MS));
  try {
    await client.sendInput(sessionId, '\r');
  } catch (e) {
    throw new NotSubmitted(e instanceof Error ? e.message : 'Could not send');
  }
}
