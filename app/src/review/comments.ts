import { useSyncExternalStore } from 'react';
import type { SessionId } from '../lib/client/types';

/** One line comment of a review: `line` is a line of the file as it is in the working tree. */
export interface ReviewComment {
  path: string;
  line: number;
  /** The line's text when the comment was added: the agent may have moved on by the time it reads this. */
  quote: string;
  text: string;
}

// Kept per session until sent, outside the component: closing the review and opening it again keeps them.
const bySession = new Map<SessionId, readonly ReviewComment[]>();
const listeners = new Set<() => void>();
const NONE: readonly ReviewComment[] = [];

const set = (sessionId: SessionId, comments: readonly ReviewComment[]) => {
  if (comments.length === 0) bySession.delete(sessionId);
  else bySession.set(sessionId, comments);
  for (const l of [...listeners]) l();
};

export const getComments = (sessionId: SessionId): readonly ReviewComment[] => bySession.get(sessionId) ?? NONE;

/** Add the comment, or replace the one on the same line of the same file. */
export function putComment(sessionId: SessionId, comment: ReviewComment): void {
  const rest = getComments(sessionId).filter((c) => c.path !== comment.path || c.line !== comment.line);
  set(sessionId, [...rest, comment]);
}

export function removeComment(sessionId: SessionId, path: string, line: number): void {
  set(
    sessionId,
    getComments(sessionId).filter((c) => c.path !== path || c.line !== line),
  );
}

export const clearComments = (sessionId: SessionId): void => set(sessionId, NONE);

/** Boxes opened but left empty are not comments: drop them (when the review closes). */
export function dropEmptyComments(sessionId: SessionId): void {
  const kept = getComments(sessionId).filter((c) => c.text.trim() !== '');
  if (kept.length !== getComments(sessionId).length) set(sessionId, kept);
}

/** The comments that say something: what the count shows and what is sent. */
export const written = (comments: readonly ReviewComment[]): ReviewComment[] => comments.filter((c) => c.text.trim() !== '');

export function useComments(sessionId: SessionId): readonly ReviewComment[] {
  return useSyncExternalStore(
    (l) => {
      listeners.add(l);
      return () => listeners.delete(l);
    },
    () => getComments(sessionId),
  );
}

const QUOTE_MAX = 200;

/** One prompt for the agent: every written comment as `path:line`, the quoted line and the comment, grouped by file. */
export function buildReviewPrompt(comments: readonly ReviewComment[]): string {
  const files = new Map<string, ReviewComment[]>();
  for (const c of written(comments)) files.set(c.path, [...(files.get(c.path) ?? []), c]);
  const blocks = [...files.values()].flatMap((list) =>
    list
      .sort((a, b) => a.line - b.line)
      .map((c) => {
        const quote = c.quote.trim();
        return [`${c.path}:${c.line}`, `> ${quote.length > QUOTE_MAX ? `${quote.slice(0, QUOTE_MAX)}…` : quote}`, c.text.trim()].join('\n');
      }),
  );
  return ['Review comments on your changes. Please address each one:', ...blocks].join('\n\n');
}
