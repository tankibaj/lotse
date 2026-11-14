import type { Session, SessionId } from '../lib/client/types';

/** The sessions of a project that share one checkout: the main one, or one worktree. */
export interface CheckoutGroup {
  key: string;
  /** The project's own checkout (also where sessions without git live). */
  main: boolean;
  branch: string;
  worktree: boolean;
  /** Changes in the checkout, from its first git session (the same directory for all of them). */
  summary: { dirty: number; added: number; removed: number } | null;
  sessions: Session[];
}

const keyOf = (s: Session): string => (s.git.is_git && s.git.worktree ? `wt:${s.git.worktree_path ?? s.git.branch}` : 'main');

/**
 * Split a project's sessions (parents before their children, as `nestChildren` leaves them) by checkout: the
 * main one first, then each worktree in order of appearance. A child follows its parent, so the tree stays whole.
 */
export function groupByCheckout(list: readonly Session[], isChild: (s: Session) => boolean): CheckoutGroup[] {
  const groupOf = new Map<SessionId, string>();
  const groups = new Map<string, CheckoutGroup>();
  const get = (key: string, s: Session): CheckoutGroup => {
    let g = groups.get(key);
    if (!g) {
      g = { key, main: key === 'main', branch: '', worktree: key !== 'main', summary: null, sessions: [] };
      groups.set(key, g);
    }
    if (!g.summary && s.git.is_git) {
      g.branch = s.git.branch || 'HEAD';
      g.summary = { dirty: s.git.dirty, added: s.git.added ?? 0, removed: s.git.removed ?? 0 };
    }
    return g;
  };
  for (const s of list) {
    const parentKey = isChild(s) && s.parent_session ? groupOf.get(s.parent_session) : undefined;
    const key = parentKey ?? keyOf(s);
    groupOf.set(s.id, key);
    get(key, s).sessions.push(s);
  }
  const main = groups.get('main');
  return [...(main ? [main] : []), ...[...groups.values()].filter((g) => !g.main)];
}
