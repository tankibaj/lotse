import type { SourceLayer, SubAgent, SubAgentId } from '../lib/client/types';

// Helpers for the data the Claude mod adds (proto 5). All of it is optional: callers render nothing when a field is absent.

/** From this share of the context window the agent is close to compacting. */
export const COMPACT_AT = 85;

export const clampPct = (pct: number): number => Math.min(100, Math.max(0, Math.round(pct)));

export const SOURCE_TITLE: Record<SourceLayer, string> = {
  mod: 'Source: mod (live data from the Claude Code mod)',
  hooks: 'Source: hooks (the agent’s hook events)',
  screen: 'Source: screen (read off the terminal)',
};

/** `$0.04`; a sub-cent cost shows `<$0.01`. */
export const formatCost = (usd: number): string => (usd > 0 && usd < 0.005 ? '<$0.01' : `$${usd.toFixed(2)}`);

export interface SubAgentRow {
  agent: SubAgent;
  /** 0 for a top-level sub-agent, 1 for one it started, and so on (capped at 3). */
  depth: number;
}

/** Parents first, each followed by the sub-agents it started. One whose parent is unknown stays at the top level. */
export function orderSubAgents(list: readonly SubAgent[]): SubAgentRow[] {
  const ids = new Set<SubAgentId>(list.map((x) => x.id));
  const rows: SubAgentRow[] = [];
  const seen = new Set<SubAgentId>();
  const add = (agent: SubAgent, depth: number) => {
    if (seen.has(agent.id)) return; // a parent cycle must not loop
    seen.add(agent.id);
    rows.push({ agent, depth: Math.min(depth, 3) });
    for (const child of list) if (child.parent === agent.id) add(child, depth + 1);
  };
  for (const x of list) if (x.parent === undefined || !ids.has(x.parent)) add(x, 0);
  for (const x of list) add(x, 0); // only a cycle is left
  return rows;
}
