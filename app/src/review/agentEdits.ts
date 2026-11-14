import type { TranscriptTurn } from '../lib/client/types';

const WRITE_TOOLS = new Set(['Edit', 'Write', 'MultiEdit', 'NotebookEdit']);

/**
 * Which sub-agents changed which files, read from the transcript's Edit/Write turns (the protocol has no per-agent
 * file list yet). A turn names a file by its path, relative or absolute; turns of the main agent carry no
 * `sub_agent` and are left out (no split). Names come in first-seen order.
 */
export function agentEdits(turns: readonly TranscriptTurn[], paths: readonly string[]): Map<string, string[]> {
  const out = new Map<string, string[]>();
  for (const t of turns) {
    if (!t.sub_agent || !t.tool || !WRITE_TOOLS.has(t.tool.name)) continue;
    const summary = t.tool.summary.trim();
    for (const p of paths) {
      if (summary !== p && !summary.endsWith(`/${p}`)) continue;
      const names = out.get(p) ?? [];
      if (!names.includes(t.sub_agent)) names.push(t.sub_agent);
      out.set(p, names);
    }
  }
  return out;
}
