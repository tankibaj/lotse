import { NotSubmitted, SUBMIT_DELAY_MS, pasteBytes } from '../composer/send';
import type { AgentKind, DaemonClient, SessionId } from '../lib/client';
import { stripControl } from './comments';

/** Agents whose TUI takes a bracketed paste as one input. A plain shell or an unknown program may not. */
const PASTE_AGENTS: readonly AgentKind[] = ['claude_code', 'codex', 'opencode'];

/**
 * A known agent gets the review prompt as one bracketed paste, then the Return as its own write. Unlike `sendPrompt` this
 * does not ask the pane's terminal whether it wants bracketed paste: with the terminal unmounted that answer is `false`
 * and the multi-line prompt would go out as one submit per line. Control bytes are stripped again here, so nothing in
 * the text can end the paste early.
 * A plain shell or unknown agent gets the prompt as a single line, without paste markers and without the Return (a line
 * break would run it as a command): it throws `NotSubmitted` so the caller tells the user to look at it and press Return.
 */
export async function sendReviewPrompt(client: Pick<DaemonClient, 'sendInput'>, sessionId: SessionId, text: string, agent: AgentKind | undefined): Promise<void> {
  const clean = stripControl(text);
  if (agent === undefined || !PASTE_AGENTS.includes(agent)) {
    await client.sendInput(sessionId, clean.replace(/\s+/g, ' ').trim());
    throw new NotSubmitted('Not an agent session: the review was typed, not submitted');
  }
  await client.sendInput(sessionId, pasteBytes(clean.replace(/\n+$/, ''), true));
  await new Promise((resolve) => setTimeout(resolve, SUBMIT_DELAY_MS));
  try {
    await client.sendInput(sessionId, '\r');
  } catch (e) {
    throw new NotSubmitted(e instanceof Error ? e.message : 'Could not send');
  }
}
