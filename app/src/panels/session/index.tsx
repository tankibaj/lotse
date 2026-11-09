import { FolderGit2, Folder, GitBranch, Laptop, Radio, SquareTerminal } from 'lucide-react';
import { useCallback, useEffect, useState } from 'react';
import type { ReactNode } from 'react';
import { useClient } from '../../lib/client';
import type { Connector, DetectSource, Session, SessionId, SessionInfo, Status, Usage } from '../../lib/client/types';
import { AGENT_LABEL, STATUS_LABEL, coarseAge, formatTokens, useCoarseNow, useWorkspace } from '../../lib/state';
import { ConfirmDialog } from '../../sidebar/ConfirmDialog';
import { SWATCHES, StatusGlyph } from '../../sidebar';
import { ProjectIcon } from '../../sheets/symbols';
import { Connectors } from './Connectors';
import { Section } from './Section';

const SOURCE_LABEL: Record<DetectSource, string> = { hook: 'Hooks', osc7501: 'OSC 7501', screen: 'Screen', process: 'Process' };
const SUB_LABEL: Partial<Record<Status, string>> = { run: 'running', perm: 'waiting', fail: 'failed', done: 'done' };

/** `undefined` while loading, `null` when the request failed. Reloads when `key` changes or `reload()` runs. */
function useLoaded<T>(load: () => Promise<T>, key: string): [T | null | undefined, () => void] {
  const [value, setValue] = useState<T | null | undefined>(undefined);
  const [n, setN] = useState(0);
  useEffect(() => {
    let live = true;
    load().then(
      (v) => live && setValue(v),
      () => live && setValue(null),
    );
    return () => {
      live = false;
    };
    // `load` closes over the client and session that `key` names
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [key, n]);
  return [value, useCallback(() => setN((x) => x + 1), [])];
}

const Row = ({ icon, children, right, title }: { icon: ReactNode; children: ReactNode; right?: ReactNode; title?: string }) => (
  <div className="flex min-w-0 items-center gap-2.5 py-1 text-[13px]" title={title}>
    <span aria-hidden className="flex h-4 w-4 shrink-0 items-center justify-center text-dim">
      {icon}
    </span>
    <span className="flex-1 truncate">{children}</span>
    {right}
  </div>
);

function Header({ session, info, now }: { session: Session; info: SessionInfo | null; now: number }) {
  const { projects } = useWorkspace();
  const project = projects.find((p) => p.id === session.project_id);
  const shell = session.agent === 'terminal';
  const elapsed = session.status === 'run' && session.running_since !== null ? ` · ${coarseAge(now - session.running_since)}` : '';
  return (
    <div data-testid="session-header" className="mb-4 flex items-center gap-2.5">
      <span
        aria-hidden
        className="grid h-7 w-7 shrink-0 place-items-center rounded-[7px] text-white"
        style={{ background: SWATCHES[project?.color ?? 'gray'] ?? SWATCHES.gray }}
      >
        <ProjectIcon name={project?.icon ?? 'folder'} size={19} strokeWidth={2.4} />
      </span>
      <div className="min-w-0 flex-1">
        <div className="truncate font-semibold">
          {AGENT_LABEL[session.agent]}
          {!shell && info?.agent_version && <span className="ml-1.5 text-[12px] font-normal text-dim">{info.agent_version}</span>}
        </div>
        <div className="flex items-center gap-1.5 text-[12px] text-dim">
          <StatusGlyph status={session.status} />
          <span>
            {STATUS_LABEL[session.status]}
            {elapsed}
            {!shell && info?.model && ` · ${info.model}`}
          </span>
        </div>
      </div>
    </div>
  );
}

function Environment({ session, info }: { session: Session; info: SessionInfo | null }) {
  const { projects } = useWorkspace();
  const project = projects.find((p) => p.id === session.project_id);
  const git = session.git;
  const worktree = info?.worktree ?? git.worktree_path;
  return (
    <Section title="Environment" testId="session-environment">
      <Row icon={<Laptop size={16} />}>{info?.machine ?? 'This computer'}</Row>
      <Row icon={<Folder size={16} />} title={info?.cwd}>
        {project?.name}
        {info && <span className="text-dim"> · {info.cwd}</span>}
      </Row>
      {!git.is_git ? (
        <Row icon={<Folder size={16} />}>No git</Row>
      ) : (
        <Row
          icon={git.worktree ? <FolderGit2 size={16} /> : <GitBranch size={16} />}
          title={git.branch}
          right={
            <span className="text-[12px] tabular-nums text-dim">
              {git.ahead > 0 && `↑${git.ahead} `}
              {git.dirty > 0 && `±${git.dirty}`}
            </span>
          }
        >
          <span className="font-mono text-[12px]">{git.branch}</span>
        </Row>
      )}
      {git.is_git && git.worktree && worktree && (
        <div className="truncate pl-[26px] font-mono text-[11px] text-dim" title={worktree}>
          worktree · {worktree}
        </div>
      )}
      <Row icon={<Radio size={16} />}>
        Detected via <span className="text-dim">{SOURCE_LABEL[session.source]}</span>
      </Row>
    </Section>
  );
}

function ContextBar({ usage }: { usage: Usage }) {
  const [open, setOpen] = useState(false);
  // cumulative session totals, not the current context size: no window, percentage, bar or combined total
  const { input, output, cache_read, cache_write } = usage.tokens;
  const cost = `$${(usage.spend.session / 1_000_000).toFixed(2)}${usage.priced ? '' : ' +'}`;
  return (
    <div className="border-t border-line py-3">
      <button className="flex w-full items-center gap-2 text-[13px]" aria-expanded={open} onClick={() => setOpen(!open)}>
        <span>Usage</span>
        <span aria-hidden className="text-[10px] text-dim">
          {open ? '▾' : '▸'}
        </span>
        <span className="ml-auto text-[12px] tabular-nums text-dim">{cost}</span>
      </button>
      {open && (
        <div className="mt-2 grid grid-cols-[1fr_auto] gap-y-1 text-[12px] text-dim">
          <span>Input</span>
          <span className="tabular-nums">{formatTokens(input)}</span>
          <span>Output</span>
          <span className="tabular-nums">{formatTokens(output)}</span>
          <span>Cache read</span>
          <span className="tabular-nums">{formatTokens(cache_read)}</span>
          <span>Cache write</span>
          <span className="tabular-nums">{formatTokens(cache_write)}</span>
          <span>Session cost</span>
          <span className="tabular-nums" title={usage.priced ? undefined : 'A model without a known price took part: the cost covers the priced part only.'}>
            {cost}
          </span>
        </div>
      )}
    </div>
  );
}

function SubAgents({ session }: { session: Session }) {
  return (
    <Section title="Sub-agents" extra={session.sub_agents.length} testId="session-subagents">
      {session.sub_agents.map((a) => (
        <div key={a.id} className="flex items-center gap-2.5 py-1 text-[13px]">
          <StatusGlyph status={a.status} />
          <span className="flex-1 truncate">{a.name}</span>
          <span className="text-[12px] tabular-nums text-dim">{SUB_LABEL[a.status]}</span>
        </div>
      ))}
    </Section>
  );
}

function Tasks({ sessionId, info, now, changed }: { sessionId: SessionId; info: SessionInfo; now: number; changed: () => void }) {
  const client = useClient();
  const [asking, setAsking] = useState<SessionInfo['tasks'][number] | null>(null);
  return (
    <Section title="Background tasks" extra={info.tasks.length}>
      {info.tasks.map((t) => (
        <div key={t.pid} className="flex items-center gap-2.5 py-1 text-[13px]">
          <SquareTerminal size={16} aria-hidden className="shrink-0 text-dim" />
          <span className="flex-1 truncate font-mono text-[12px]" title={t.command}>
            {t.command}
          </span>
          <span className="text-[12px] text-dim">{coarseAge(now - t.started_at)}</span>
          <button className="grid h-4 w-4 place-items-center" aria-label={`Stop task ${t.command}`} title="Stop" onClick={() => setAsking(t)}>
            <span className="block h-2.5 w-2.5 rounded-[2px] bg-dim" />
          </button>
        </div>
      ))}
      {asking && (
        <ConfirmDialog
          title={`Stop ${asking.command}?`}
          body={`Sends SIGTERM to process ${asking.pid}.`}
          confirmLabel="Stop"
          onCancel={() => setAsking(null)}
          onConfirm={() => {
            setAsking(null);
            // the task may be gone already (`not_found`): either way the list is stale
            client.stopTask(sessionId, asking.pid).catch(() => {}).finally(changed);
          }}
        />
      )}
    </Section>
  );
}

/** Configured servers (`list_connectors`), or the live check when Settings › External allows it; Retry replaces the list. */
function useConnectors(sessionId: SessionId, live: boolean): [Connector[] | null | undefined, (c: Connector[]) => void] {
  const client = useClient();
  const [list, setList] = useState<Connector[] | null | undefined>(undefined);
  useEffect(() => {
    let current = true;
    (live ? client.checkConnectors(sessionId) : client.listConnectors(sessionId)).then(
      (c) => current && setList(c),
      () => current && setList(null),
    );
    return () => {
      current = false;
    };
  }, [client, sessionId, live]);
  return [list, setList];
}

/** Keyed by session in `SessionPanel`, so nothing of the previous session shows while the next one loads. */
function Panel({ session }: { session: Session }) {
  const client = useClient();
  const { settings } = useWorkspace();
  const live = settings?.external.mcp_live ?? false;
  // status and sub-agent changes are the daemon's cue that tasks, tokens or the model moved
  const key = `${session.status}|${session.sub_agents.map((a) => `${a.id}${a.status}`).join(',')}`;
  const [info, reloadInfo] = useLoaded(() => client.getSessionInfo(session.id), key);
  const [usage] = useLoaded(() => client.getUsage(session.id), key);
  const [connectors, setConnectors] = useConnectors(session.id, live);
  const now = useCoarseNow(
    (t) => [session.running_since, ...(info?.tasks.map((x) => x.started_at) ?? [])].map((from) => (from === null ? '' : coarseAge(t - from))).join('|'),
    () => false,
  );
  if (info === undefined) return null;
  const agent = session.agent !== 'terminal';
  return (
    <div data-testid="panel-session" data-session-id={session.id} className="text-[13px]">
      <Header session={session} info={info} now={now} />
      <div>
        <Environment session={session} info={info} />
        {agent && usage && <ContextBar usage={usage} />}
        {session.sub_agents.length > 0 && <SubAgents session={session} />}
        {agent && connectors && (
          <Connectors
            sessionId={session.id}
            connectors={connectors}
            live={live}
            retry={() => void client.checkConnectors(session.id).then(setConnectors, () => {})}
          />
        )}
        {info && info.tasks.length > 0 && <Tasks sessionId={session.id} info={info} now={now} changed={reloadInfo} />}
      </div>
    </div>
  );
}

export function SessionPanel({ sessionId }: { sessionId: SessionId | null }) {
  const session = useWorkspace().sessions.find((s) => s.id === sessionId);
  if (!session) {
    return (
      <div data-testid="panel-session" className="text-[13px] text-dim">
        No session selected.
      </div>
    );
  }
  return <Panel key={session.id} session={session} />;
}
