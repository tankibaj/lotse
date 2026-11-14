import type { PrInfo, PrState } from '../../lib/client/types';

// Shared by the Changes panel and the session card.

const STATE_LABEL: Record<PrState, string> = { open: 'Open', draft: 'Draft', merged: 'Merged', closed: 'Closed' };

/** Dot colour of the pill (`spec/mock.html`: gray for a draft). */
export const PR_DOT: Record<PrState, string> = { open: 'bg-green', draft: 'bg-gray', merged: 'bg-c-purple', closed: 'bg-red' };

/** `PR #184 · Draft`; GitLab calls them merge requests: `MR !184 · Open`. */
export const prLabel = (pr: PrInfo): string => `${pr.provider === 'gitlab' ? 'MR !' : 'PR #'}${pr.number} · ${STATE_LABEL[pr.state]}`;

/** `Pull request #184 · Draft`, for tooltips. */
export const prTitle = (pr: PrInfo): string =>
  `${pr.provider === 'gitlab' ? 'Merge request !' : 'Pull request #'}${pr.number} · ${STATE_LABEL[pr.state]}`;

/** What the user pastes into a terminal in the main checkout. A branch name can hold shell characters (`;`, `$`, `&`), so it is quoted unless plain. */
export const mergeCommand = (branch: string): string =>
  /^[\w./@+-]+$/.test(branch) ? `git merge ${branch}` : `git merge '${branch.replaceAll("'", `'\\''`)}'`;
