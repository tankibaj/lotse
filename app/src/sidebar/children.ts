import type { Session, SessionId } from '../lib/client/types';

/**
 * Child sessions (proto 5 `parent_session`) sit right under their parent, in the daemon's order, when the parent
 * is in the same list. A child whose parent is elsewhere (another project) or closed stays where it is: top level.
 * Depth is capped at 2 by the daemon; a cycle cannot hang this (each session is placed once).
 */
export function nestChildren(list: readonly Session[]): { session: Session; child: boolean }[] {
  const ids = new Set(list.map((s) => s.id));
  const kids = new Map<SessionId, Session[]>();
  for (const s of list) {
    if (s.parent_session && s.parent_session !== s.id && ids.has(s.parent_session)) {
      kids.set(s.parent_session, [...(kids.get(s.parent_session) ?? []), s]);
    }
  }
  const placed = new Set<SessionId>();
  const out: { session: Session; child: boolean }[] = [];
  const place = (s: Session, child: boolean) => {
    if (placed.has(s.id)) return;
    placed.add(s.id);
    out.push({ session: s, child });
    for (const k of kids.get(s.id) ?? []) place(k, true);
  };
  for (const s of list) if (!isChild(s, ids)) place(s, false);
  // anything left sits in a parent cycle: show it flat
  for (const s of list) place(s, false);
  return out;
}

const isChild = (s: Session, ids: ReadonlySet<SessionId>) => !!s.parent_session && s.parent_session !== s.id && ids.has(s.parent_session);

/** Children of `parent` that are done or failed: the badge on the parent's card. */
export function childOutcome(parent: SessionId, all: readonly Session[]): { done: number; fail: number } {
  let done = 0;
  let fail = 0;
  for (const s of all) {
    if (s.parent_session !== parent) continue;
    if (s.status === 'done') done++;
    else if (s.status === 'fail') fail++;
  }
  return { done, fail };
}
