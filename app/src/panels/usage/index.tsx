import type { ReactNode } from 'react';
import type { SessionId, Status, Usage, UsageLane } from '../../lib/client/types';
import { AGENT_LABEL, coarseAge, formatTokens, useWorkspace } from '../../lib/state';
import { useUsage } from './useUsage';

const MIN = 60_000;

const COLOR: Record<Status, string> = {
  run: 'var(--blue)',
  perm: 'var(--orange)',
  done: 'var(--green)',
  fail: 'var(--red)',
  idle: 'var(--gray)',
  unknown: 'var(--gray)',
};
const LABEL: Record<Status, string> = {
  run: 'Working',
  perm: 'Waiting on you',
  done: 'Done',
  fail: 'Failed',
  idle: 'Idle',
  unknown: 'Unknown',
};
const LEGEND_ORDER: Status[] = ['run', 'perm', 'done', 'fail', 'idle', 'unknown'];

const duration = (ms: number): string => (ms <= 0 ? '0m' : coarseAge(ms));
const dollars = (micros: number): string => `$${(micros / 1_000_000).toFixed(2)}`;
const pct = (part: number, whole: number): string => `${((part / whole) * 100).toFixed(2)}%`;
const clock = (ms: number): string => {
  const d = new Date(Math.round(ms / MIN) * MIN);
  return `${String(d.getHours()).padStart(2, '0')}:${String(d.getMinutes()).padStart(2, '0')}`;
};

/** A lane lasts from its first span's start to its last span's end. */
const laneMs = (lane: UsageLane): number =>
  lane.spans.length === 0 ? 0 : Math.max(...lane.spans.map((s) => s.to)) - Math.min(...lane.spans.map((s) => s.from));

function Tile({ id, label, value, sub, color }: { id: string; label: string; value: string; sub: string; color?: string }) {
  return (
    <div data-testid={`usage-tile-${id}`} className="rounded-lg bg-win px-2.5 py-2 ring-1 ring-black/5 dark:ring-white/10">
      <div className="text-[11px] text-dim">{label}</div>
      <div className="mt-0.5 text-[17px] font-semibold tabular-nums" style={color ? { color } : undefined}>
        {value}
      </div>
      <div className="text-[10px] text-dim">{sub}</div>
    </div>
  );
}

function Section({ title, right, testId, children }: { title: string; right?: string; testId?: string; children: ReactNode }) {
  return (
    <section data-testid={testId}>
      <div className="mb-2 mt-5 flex items-baseline">
        <h4 className="flex-1 text-[11px] font-semibold uppercase tracking-wide text-dim">{title}</h4>
        {right && <span className="text-[11px] text-dim">{right}</span>}
      </div>
      {children}
    </section>
  );
}

function Timeline({ usage, priced }: { usage: Usage; priced: boolean }) {
  const { started_at: start, lanes } = usage;
  const span = Math.max(usage.total_ms, ...lanes.flatMap((l) => l.spans.map((s) => s.to - start)), 1);
  const present = LEGEND_ORDER.filter((st) => lanes.some((l) => l.spans.some((s) => s.status === st)));
  return (
    <Section title="Timeline" right={`${clock(start)} – now`}>
      <div className="relative text-[11px]">
        {lanes.map((lane, i) => (
          <div key={`${lane.name}:${i}`} data-testid="usage-lane" data-kind={lane.kind} className="flex h-7 items-center gap-2">
            <span className="w-[84px] shrink-0 truncate" title={lane.name}>
              {lane.name}
            </span>
            <div
              role="img"
              aria-label={`${lane.name}: ${lane.spans.map((s) => `${LABEL[s.status]} ${duration(s.to - s.from)}`).join(', ') || 'no activity'}`}
              className="relative h-2.5 flex-1 overflow-hidden rounded-full bg-black/5 dark:bg-white/10"
            >
              {lane.spans.map((s, j) => (
                <div
                  key={j}
                  data-testid="usage-span"
                  data-status={s.status}
                  className="absolute top-0 h-full"
                  style={{ left: pct(s.from - start, span), width: pct(s.to - s.from, span), backgroundColor: COLOR[s.status] }}
                  title={`${LABEL[s.status]} · ${duration(s.to - s.from)}`}
                />
              ))}
            </div>
            <span className="w-[70px] shrink-0 text-right tabular-nums text-dim">
              {duration(laneMs(lane))}
              {priced && ` · ${dollars(lane.cost_micros)}`}
            </span>
          </div>
        ))}
        <div data-testid="usage-axis" className="ml-[92px] mr-[78px] mt-0.5 flex justify-between text-[10px] tabular-nums text-dim">
          <span>{clock(start)}</span>
          <span>{clock(start + span / 3)}</span>
          <span>{clock(start + (span * 2) / 3)}</span>
          <span>now</span>
        </div>
      </div>
      <div className="mt-2 flex flex-wrap gap-x-3 gap-y-1 text-[10px] text-dim">
        {present.map((st) => (
          <span key={st} className="inline-flex items-center gap-1">
            <span className="h-2 w-2 rounded-full" style={{ backgroundColor: COLOR[st] }} />
            {LABEL[st]}
          </span>
        ))}
      </div>
    </Section>
  );
}

function Tokens({ usage }: { usage: Usage }) {
  const { tokens, priced, spend } = usage;
  const parts = [
    { label: 'Input', n: tokens.input, color: 'var(--blue)' },
    { label: 'Cache read', n: tokens.cache_read, color: 'var(--c-teal,#5ac8be)' },
    { label: 'Cache write', n: tokens.cache_write, color: 'var(--c-indigo)' },
    { label: 'Output', n: tokens.output, color: 'var(--c-lavender,#b4a7e5)' },
  ].filter((p) => p.n > 0);
  const total = parts.reduce((sum, p) => sum + p.n, 0);
  if (total === 0) {
    return (
      <Section title="Tokens" testId="usage-tokens">
        <div className="text-[12px] text-dim">No token data</div>
      </Section>
    );
  }
  return (
    <Section title="Tokens" right={priced ? `${formatTokens(total)} · ${dollars(spend.session)}` : formatTokens(total)} testId="usage-tokens">
      <div
        role="img"
        aria-label={parts.map((p) => `${p.label} ${formatTokens(p.n)}`).join(', ')}
        className="flex h-2 gap-px overflow-hidden rounded-full"
      >
        {parts.map((p) => (
          <div key={p.label} style={{ width: pct(p.n, total), backgroundColor: p.color }} title={`${p.label} ${formatTokens(p.n)}`} />
        ))}
      </div>
      <div className="mt-2 grid grid-cols-3 text-[11px]">
        {parts.map((p) => (
          <div key={p.label}>
            <span className="inline-flex items-center gap-1 text-dim">
              <span className="h-2 w-2 rounded-full" style={{ backgroundColor: p.color }} />
              {p.label}
            </span>
            <div className="font-medium tabular-nums">{formatTokens(p.n)}</div>
          </div>
        ))}
      </div>
      {!priced && <div className="mt-2 text-[11px] text-dim">Tokens only: a model without a known price took part.</div>}
    </Section>
  );
}

function Spend({ usage, projectName }: { usage: Usage; projectName: string }) {
  const { session, project, all } = usage.spend;
  if (!usage.priced || session + project + all === 0) return null;
  const max = Math.max(all, project, session, 1);
  const rows: [string, number][] = [
    ['This session', session],
    [projectName, project],
    ['All projects', all],
  ];
  return (
    <Section title="Spend today" testId="usage-spend">
      <div className="space-y-2.5 text-[12px]">
        {rows.map(([label, micros]) => (
          <div key={label}>
            <div className="mb-1 flex">
              <span className="flex-1 truncate">{label}</span>
              <span className="tabular-nums">{dollars(micros)}</span>
            </div>
            <div className="h-1.5 overflow-hidden rounded-full bg-black/5 dark:bg-white/10">
              <div className="h-full rounded-full" style={{ width: pct(micros, max), backgroundColor: 'var(--gray)' }} />
            </div>
          </div>
        ))}
      </div>
    </Section>
  );
}

function Body({ usage, projectName }: { usage: Usage; projectName: string }) {
  const { total_ms: total, working_ms: working, waiting_ms: waiting } = usage;
  const whole = Math.max(total, 1);
  const split = `Working ${duration(working)} · Waiting on you ${duration(waiting)}`;
  return (
    <>
      <div className="grid grid-cols-3 gap-2">
        <Tile id="session" label="Session" value={duration(total)} sub={`since ${clock(usage.started_at)}`} />
        <Tile id="working" label="Working" value={duration(working)} sub="agent active" />
        <Tile id="waiting" label="Waiting" value={duration(waiting)} sub="on you" color="var(--orange)" />
      </div>
      <div data-testid="usage-split" role="img" aria-label={split} title={split} className="mt-2 flex h-1.5 overflow-hidden rounded-full">
        <div style={{ width: pct(working, whole), backgroundColor: 'var(--blue)' }} />
        <div style={{ width: pct(waiting, whole), backgroundColor: 'var(--orange)' }} />
      </div>
      <Timeline usage={usage} priced={usage.priced && usage.tokens.input + usage.tokens.output + usage.tokens.cache_read + usage.tokens.cache_write > 0} />
      <Tokens usage={usage} />
      <Spend usage={usage} projectName={projectName} />
      <div className="mt-4 text-[11px] text-dim">Costs are estimates from local transcripts. Nothing leaves this Mac.</div>
    </>
  );
}

/** The mock's `usagePanel`: time tiles and split, a timeline lane per agent and sub-agent, tokens, spend today. */
export function UsagePanel({ sessionId }: { sessionId: SessionId | null }) {
  const state = useUsage(sessionId);
  const { sessions, projects } = useWorkspace();
  const session = sessions.find((s) => s.id === sessionId);
  const projectName = projects.find((p) => p.id === session?.project_id)?.name ?? 'This project';
  const model = state.kind === 'ready' ? state.usage.model : null;
  const sub = [session?.title, session && AGENT_LABEL[session.agent], model].filter(Boolean).join(' · ');
  return (
    <div data-testid="panel-usage" data-session-id={sessionId ?? undefined} className="text-[13px]">
      <div className="mb-3 min-w-0">
        <h3 className="font-semibold">Usage &amp; time</h3>
        {sub && <div className="mt-0.5 truncate text-[11px] text-dim">{sub}</div>}
      </div>
      {state.kind === 'none' && <div className="text-[12px] text-dim">No session selected</div>}
      {state.kind === 'loading' && <div className="text-[12px] text-dim">Loading…</div>}
      {state.kind === 'error' && <div className="text-[12px] text-dim">Usage unavailable: {state.message}</div>}
      {state.kind === 'ready' && <Body usage={state.usage} projectName={projectName} />}
    </div>
  );
}
