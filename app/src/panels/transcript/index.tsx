import { Bot, FileText, Pencil, Search, Share, SquareTerminal, Wrench } from 'lucide-react';
import type { LucideIcon } from 'lucide-react';
import { useCallback, useEffect, useLayoutEffect, useMemo, useRef, useState } from 'react';
import { useClient } from '../../lib/client';
import type { ExportFormat, SessionId, TranscriptTurn } from '../../lib/client/types';
import { AGENT_LABEL, displayStatus, useWorkspace } from '../../lib/state';
import { StatusGlyph } from '../../panes/StatusGlyph';
import { useLoad } from '../../sheets/useLoad';
import { exportBridge } from './exportBridge';
import { approvalDetail, approvalTitle, buildItems, clock, counts } from './model';
import type { Filter, Item, ToolRow } from './model';

// Show Transcript (⇧⌘T): the right panel of `spec/mock.html` trPanel. Read-only view of `get_transcript`
// (Claude Code, Codex, or a terminal's scrollback) with filters, search, a Working bar, and the Export menu.

const ICON: Record<string, LucideIcon> = { Agent: Bot, Read: FileText, Update: Pencil, Edit: Pencil, Write: Pencil, Bash: SquareTerminal, Grep: Search };
const FORMATS: { format: ExportFormat; label: string }[] = [
  { format: 'markdown', label: 'Markdown (.md)' },
  { format: 'json_lines', label: 'JSON Lines (.jsonl)' },
  { format: 'text', label: 'Terminal text (.txt)' },
];
const REFRESH_MS = 3000;
const AT_BOTTOM_PX = 24;

const baseName = (p: string) => p.split(/[\\/]/).pop() || p;
const plural = (n: number, w: string) => `${n} ${w}${n === 1 ? '' : 's'}`;
const messageOf = (e: unknown) => (e instanceof Error ? e.message : typeof e === 'string' ? e : 'Something went wrong');

/** The ancestor that scrolls (the right panel's `aside`). */
function scrollParent(el: HTMLElement): HTMLElement {
  for (let p = el.parentElement; p; p = p.parentElement) {
    const y = getComputedStyle(p).overflowY;
    if (y === 'auto' || y === 'scroll') return p;
  }
  return document.documentElement;
}

function ToolCard({ rows }: { rows: ToolRow[] }) {
  return (
    <div data-testid="tool-group" className="divide-y divide-line overflow-hidden rounded-lg bg-win ring-1 ring-black/5 dark:ring-white/10">
      {rows.map((r) => {
        const Icon = ICON[r.label] ?? Wrench;
        return (
          <div key={r.key} data-testid="tool-row" title={clock(r.at)} className="flex h-8 min-w-0 items-center gap-2 px-2.5 text-[12px]">
            <Icon className="h-3.5 w-3.5 shrink-0 text-dim" aria-hidden />
            <span className="shrink-0 font-medium">{r.label}</span>
            <span className="min-w-0 flex-1 truncate font-mono text-[11px] text-dim" title={r.arg}>
              {r.arg}
            </span>
            <span className="max-w-[40%] shrink-0 truncate text-[11px] tabular-nums text-dim" title={r.result}>
              {r.status === 'run' ? '' : r.result}
            </span>
            <StatusGlyph status={r.status} />
          </div>
        );
      })}
    </div>
  );
}

function ApprovalCard({ turn, approval }: Pick<Extract<Item, { kind: 'approval' }>, 'turn' | 'approval'>) {
  return (
    <div data-testid="approval-card" className="flex items-start gap-2 rounded-lg bg-orange/10 px-2.5 py-2">
      <StatusGlyph status="perm" />
      <div className="min-w-0 flex-1">
        <div className="flex text-[12px]">
          <span className="flex-1 font-medium">{approvalTitle(approval)}</span>
          <span className="text-[11px] text-dim">{clock(turn.at)}</span>
        </div>
        <div className="truncate font-mono text-[11px]" title={approval.command}>
          {approval.command}
        </div>
        <div className="text-[11px] text-dim">{approvalDetail(approval)}</div>
      </div>
    </div>
  );
}

function Message({ turn, agent }: { turn: TranscriptTurn; agent: string }) {
  if (turn.role === 'user') {
    return (
      <div data-role="user" className="flex flex-col items-end">
        <div className="mb-1 text-[11px] text-dim">You · {clock(turn.at)}</div>
        <div className="max-w-[88%] whitespace-pre-wrap break-words rounded-2xl rounded-tr-md bg-sel px-3 py-2">{turn.text}</div>
      </div>
    );
  }
  return (
    <div data-role="assistant">
      <div className="mb-1 flex items-center gap-1 text-[11px] text-dim">
        <span className="h-1.5 w-1.5 rounded-full" style={{ background: 'var(--c-tan, #c8a27a)' }} />
        {agent} · {clock(turn.at)}
      </div>
      <div className="whitespace-pre-wrap break-words leading-relaxed">{turn.text}</div>
    </div>
  );
}

function ExportMenu({ sessionId, onDone }: { sessionId: SessionId; onDone: (n: { ok: boolean; text: string } | null) => void }) {
  const client = useClient();
  const [open, setOpen] = useState(false);
  const [busy, setBusy] = useState(false);
  const root = useRef<HTMLDivElement>(null);

  useEffect(() => {
    if (!open) return;
    const away = (e: MouseEvent) => root.current && !root.current.contains(e.target as Node) && setOpen(false);
    const esc = (e: KeyboardEvent) => e.key === 'Escape' && setOpen(false);
    document.addEventListener('mousedown', away);
    document.addEventListener('keydown', esc);
    return () => {
      document.removeEventListener('mousedown', away);
      document.removeEventListener('keydown', esc);
    };
  }, [open]);

  const run = async (format: ExportFormat) => {
    setOpen(false);
    setBusy(true);
    onDone(null);
    try {
      const doc = await client.exportTranscript(sessionId, format);
      const path = await exportBridge().save({ fileName: doc.file_name, content: doc.content });
      if (path !== null) onDone({ ok: true, text: `Saved ${baseName(path)} · ${plural(doc.redactions, 'secret')} redacted` });
    } catch (e) {
      onDone({ ok: false, text: messageOf(e) });
    } finally {
      setBusy(false);
    }
  };

  return (
    <div ref={root} className="relative">
      <button
        type="button"
        aria-haspopup="menu"
        aria-expanded={open}
        disabled={busy}
        onClick={() => setOpen((o) => !o)}
        className="flex h-7 items-center gap-1 rounded-md px-2 text-[12px] hover:bg-black/5 disabled:opacity-50 dark:hover:bg-white/10"
      >
        <Share className="h-3.5 w-3.5" aria-hidden />
        Export
      </button>
      {open && (
        <div role="menu" aria-label="Export transcript" className="absolute right-0 z-10 mt-1 w-[220px] rounded-lg bg-win p-1 text-[12px] shadow-xl ring-1 ring-black/10">
          {FORMATS.map((f, i) => (
            <button key={f.format} type="button" role="menuitem" autoFocus={i === 0} onClick={() => void run(f.format)} className="w-full rounded px-2 py-1 text-left hover:bg-sel focus:bg-sel focus:outline-none">
              {f.label}
            </button>
          ))}
          <div className="px-2 pb-1 pt-1.5 text-[11px] text-dim">Saved locally. Secrets redacted.</div>
        </div>
      )}
    </div>
  );
}

function Loaded({ sessionId }: { sessionId: SessionId }) {
  const client = useClient();
  const { sessions } = useWorkspace();
  const session = sessions.find((s) => s.id === sessionId);
  const working = session ? displayStatus(session) === 'run' : false;
  const agent = session ? AGENT_LABEL[session.agent] : 'Agent';
  const [state, reload] = useLoad(() => client.getTranscript(sessionId));
  const [model, setModel] = useState<string | null>(null);
  const [filter, setFilter] = useState<Filter>('all');
  const [query, setQuery] = useState('');
  const [notice, setNotice] = useState<{ ok: boolean; text: string } | null>(null);
  const root = useRef<HTMLDivElement>(null);
  const searchBox = useRef<HTMLInputElement>(null);
  const scroller = useRef<HTMLElement | null>(null);
  /** The reader is at the newest turn: new turns scroll into view. */
  const following = useRef(true);

  useEffect(() => {
    let live = true;
    client.getSessionInfo(sessionId).then((i) => live && setModel(i.model), () => {});
    return () => {
      live = false;
    };
  }, [client, sessionId]);

  // new turns arrive with the agent's work: refresh on its events, and while it runs
  useEffect(() => {
    let timer: ReturnType<typeof setTimeout> | undefined;
    const off = client.onEvent((e) => {
      const d = e.data as { session_id?: SessionId };
      if (d.session_id !== sessionId || !['status_changed', 'sub_agents_changed', 'sub_agent_status'].includes(e.event)) return;
      clearTimeout(timer);
      timer = setTimeout(reload, 300);
    });
    const poll = working ? setInterval(reload, REFRESH_MS) : undefined;
    return () => {
      off();
      clearTimeout(timer);
      clearInterval(poll);
    };
  }, [client, sessionId, working, reload]);

  useEffect(() => {
    if (!notice?.ok) return;
    const t = setTimeout(() => setNotice(null), 6000);
    return () => clearTimeout(t);
  }, [notice]);

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (!e.metaKey || e.shiftKey || e.altKey || e.ctrlKey || e.key.toLowerCase() !== 'f') return;
      e.preventDefault();
      searchBox.current?.focus();
      searchBox.current?.select();
    };
    document.addEventListener('keydown', onKey);
    return () => document.removeEventListener('keydown', onKey);
  }, []);

  useEffect(() => {
    if (!root.current) return;
    const el = scrollParent(root.current);
    scroller.current = el;
    const onScroll = () => {
      following.current = el.scrollHeight - el.scrollTop - el.clientHeight <= AT_BOTTOM_PX;
    };
    el.addEventListener('scroll', onScroll, { passive: true });
    return () => el.removeEventListener('scroll', onScroll);
  }, []);

  const scrollToLatest = useCallback(() => {
    following.current = true;
    if (scroller.current) scroller.current.scrollTop = scroller.current.scrollHeight;
  }, []);

  const data = state.status === 'ready' && state.data.available ? state.data : null;
  const turns = data?.turns;
  const items = useMemo(() => (turns ? buildItems(turns, filter, query) : []), [turns, filter, query]);
  const n = useMemo(() => counts(turns ?? []), [turns]);

  // the newest turn stays in view while the reader is at the bottom
  useLayoutEffect(() => {
    if (following.current) scrollToLatest();
  }, [items, scrollToLatest]);

  const started = clock(turns?.find((t) => t.at !== null)?.at ?? null);
  const sub = [agent, model, data?.source === 'scrollback' ? 'terminal scrollback' : null, started && `started ${started}`].filter(Boolean).join(' · ');
  const running = items
    .flatMap((i) => (i.kind === 'tools' ? i.rows : []))
    .filter((r) => r.status === 'run')
    .at(-1);
  const tabs: [Filter, string][] = [
    ['all', 'All'],
    ['messages', `Messages ${n.message}`],
    ['tools', `Tools ${n.tool}`],
    ['approvals', `Approvals ${n.approval}`],
  ];

  return (
    <div ref={root} className="-mb-4 flex h-full flex-col">
      <div className="mb-3 flex items-start gap-2">
        <div className="min-w-0 flex-1">
          <h3 className="font-semibold">Transcript</h3>
          <div className="mt-0.5 truncate text-[11px] text-dim" title={sub}>
            {sub}
          </div>
        </div>
        {data && data.turns.length > 0 && <ExportMenu sessionId={sessionId} onDone={setNotice} />}
      </div>
      {notice && (
        <div role={notice.ok ? 'status' : 'alert'} className={`mb-3 rounded-md bg-sel px-2.5 py-1.5 text-[12px] ${notice.ok ? '' : 'text-red'}`}>
          {notice.text}
        </div>
      )}
      <label className="mb-3 flex h-7 items-center gap-1.5 rounded-md bg-win px-2 ring-1 ring-black/5 dark:ring-white/10">
        <Search className="h-3.5 w-3.5 shrink-0 text-dim" aria-hidden />
        <input
          ref={searchBox}
          role="searchbox"
          aria-label="Search transcript"
          placeholder="Search transcript"
          value={query}
          onChange={(e) => setQuery(e.target.value)}
          className="min-w-0 flex-1 bg-transparent text-[12px] outline-none"
        />
        <kbd className="text-[10px] text-dim">⌘F</kbd>
      </label>
      <div className="mb-3">
        <div role="tablist" className="inline-flex rounded-md bg-black/5 p-0.5 text-[11px] dark:bg-white/10">
          {tabs.map(([k, label]) => (
            <button
              key={k}
              type="button"
              role="tab"
              aria-selected={k === filter}
              onClick={() => setFilter(k)}
              className={`rounded px-2 py-0.5 ${k === filter ? 'bg-win font-medium shadow-sm' : 'text-dim'}`}
            >
              {label}
            </button>
          ))}
        </div>
      </div>

      <div className="flex-1 space-y-3 text-[13px]">
        {state.status === 'loading' && <div className="py-6 text-center text-[12px] text-dim">Loading transcript…</div>}
        {state.status === 'error' && (
          <div role="alert" className="py-6 text-center text-[12px] text-red">
            {state.message}
          </div>
        )}
        {state.status === 'ready' && !data && (
          <div className="py-6 text-center text-[12px] text-dim">No transcript yet. {agent} reports it with its first hook event, once a prompt has run.</div>
        )}
        {data && data.turns.length === 0 && <div className="py-6 text-center text-[12px] text-dim">The transcript has no turns yet.</div>}
        {data && data.truncated && <div className="text-center text-[11px] text-dim">Older turns are not shown.</div>}
        {data && data.turns.length > 0 && items.length === 0 && <div className="py-6 text-center text-[12px] text-dim">No matches</div>}
        {items.map((i) =>
          i.kind === 'tools' ? <ToolCard key={i.key} rows={i.rows} /> : i.kind === 'approval' ? <ApprovalCard key={i.key} turn={i.turn} approval={i.approval} /> : <Message key={i.key} turn={i.turn} agent={agent === 'Claude Code' ? 'Claude' : agent} />,
        )}
      </div>

      {working && (
        <div data-testid="working-bar" className="sticky bottom-0 -mx-4 mt-3 flex items-center gap-2 border-t border-line bg-side px-4 py-2 text-[12px]">
          <StatusGlyph status="run" />
          <span className="flex-1 truncate">Working{running ? ` · ${running.label === 'Agent' ? running.arg : running.label}` : ''}</span>
          <button type="button" onClick={scrollToLatest} className="rounded-md px-2 py-0.5 text-[12px] font-medium text-blue hover:bg-sel">
            Latest ↓
          </button>
        </div>
      )}
    </div>
  );
}

export function TranscriptPanel({ sessionId }: { sessionId: SessionId | null }) {
  if (sessionId === null) {
    return (
      <div data-testid="panel-transcript" className="py-6 text-center text-[12px] text-dim">
        No session selected
      </div>
    );
  }
  return (
    <div data-testid="panel-transcript" data-session-id={sessionId} className="h-full">
      <Loaded key={sessionId} sessionId={sessionId} />
    </div>
  );
}
