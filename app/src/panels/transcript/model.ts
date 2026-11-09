import type { PermDecision, ToolStatus, TranscriptApproval, TranscriptTurn } from '../../lib/client/types';

// What the panel shows of `get_transcript`: turns sorted into messages, tool rows and approvals, filtered by the
// tabs and the search box, with consecutive tool rows folded into one card (`spec/mock.html` trPanel).

export type Filter = 'all' | 'messages' | 'tools' | 'approvals';
export type Kind = 'message' | 'tool' | 'approval';

export interface ToolRow {
  key: number;
  /** `Agent` for a sub-agent, else the tool's name. */
  label: string;
  arg: string;
  result: string;
  status: ToolStatus;
  at: number | null;
}

export type Item =
  | { kind: 'message'; key: number; turn: TranscriptTurn }
  | { kind: 'approval'; key: number; turn: TranscriptTurn; approval: TranscriptApproval }
  | { kind: 'tools'; key: number; rows: ToolRow[] };

const AGENT_TOOLS = ['Task', 'Agent'];

/** Where a turn belongs. A sub-agent's report (an assistant turn that names its sub-agent) is an Agent row. */
export function kindOf(t: TranscriptTurn): Kind | null {
  if (t.role === 'approval') return t.approval ? 'approval' : null;
  if (t.role === 'tool') return t.tool ? 'tool' : null;
  return t.sub_agent !== undefined && t.role === 'assistant' ? 'tool' : 'message';
}

export function toolRow(t: TranscriptTurn, key: number): ToolRow {
  if (t.role === 'tool' && t.tool) {
    const agent = t.sub_agent !== undefined || AGENT_TOOLS.includes(t.tool.name);
    return {
      key,
      label: agent ? 'Agent' : t.tool.name,
      arg: t.sub_agent ?? t.tool.summary,
      result: t.tool.result ?? '',
      status: t.tool.status ?? 'done',
      at: t.at,
    };
  }
  // a report reads "completed: 12 files" or "failed"
  return { key, label: 'Agent', arg: t.sub_agent ?? '', result: t.text, status: /^failed\b/i.test(t.text) ? 'fail' : 'done', at: t.at };
}

const haystack = (t: TranscriptTurn): string =>
  [t.text, t.sub_agent, t.tool?.name, t.tool?.summary, t.tool?.result, t.approval?.command].filter(Boolean).join('\n').toLowerCase();

export function counts(turns: readonly TranscriptTurn[]): Record<Kind, number> {
  const n: Record<Kind, number> = { message: 0, tool: 0, approval: 0 };
  for (const t of turns) {
    const k = kindOf(t);
    if (k) n[k]++;
  }
  return n;
}

const FILTER_KIND: Record<Exclude<Filter, 'all'>, Kind> = { messages: 'message', tools: 'tool', approvals: 'approval' };

/** The turns the tab and the search keep, in order, as items: runs of tool rows become one `tools` item. */
export function buildItems(turns: readonly TranscriptTurn[], filter: Filter, query: string): Item[] {
  const q = query.trim().toLowerCase();
  const items: Item[] = [];
  turns.forEach((turn, key) => {
    const kind = kindOf(turn);
    if (!kind || (filter !== 'all' && FILTER_KIND[filter] !== kind) || (q && !haystack(turn).includes(q))) return;
    if (kind === 'tool') {
      const last = items[items.length - 1];
      const row = toolRow(turn, key);
      if (last?.kind === 'tools') last.rows.push(row);
      else items.push({ kind: 'tools', key, rows: [row] });
    } else if (kind === 'approval' && turn.approval) {
      items.push({ kind: 'approval', key, turn, approval: turn.approval });
    } else {
      items.push({ kind: 'message', key, turn });
    }
  });
  return items;
}

const VERB: Record<PermDecision, string> = { allow: 'Allowed', always: 'Always allowed', deny: 'Denied' };
const TITLE: Record<PermDecision, string> = { allow: 'Permission allowed', always: 'Permission always allowed', deny: 'Permission denied' };

export const approvalTitle = (a: TranscriptApproval): string => TITLE[a.decision];

/** "Allowed by you · notification", or "Allowed in the terminal" when the decision was made there. */
export const approvalDetail = (a: TranscriptApproval): string => (a.via ? `${VERB[a.decision]} by you · ${a.via}` : `${VERB[a.decision]} in the terminal`);

export const clock = (at: number | null): string =>
  at === null ? '' : new Date(at).toLocaleTimeString([], { hour: '2-digit', minute: '2-digit', hour12: false });
