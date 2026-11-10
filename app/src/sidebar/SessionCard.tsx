import { Bot, FolderGit2, GitBranch, RotateCcw, Sparkles, SquareTerminal } from 'lucide-react';
import type { LucideIcon } from 'lucide-react';
import { memo } from 'react';
import type { MouseEvent as ReactMouseEvent } from 'react';
import type { PermDecision, Project, Session, SessionId } from '../lib/client/types';
import { AGENT_LABEL, displayStatus, formatTokens, noLiveStatus, sessionAge, shortBranch } from '../lib/state/selectors';
import { prLabel, prTitle } from '../panels/changes/pr';
import { InlineEdit } from './InlineEdit';
import { clampPct, COMPACT_AT, formatCost, orderSubAgents, SOURCE_TITLE } from './modData';
import { StatusGlyph } from './StatusGlyph';

// Session card, spec/research.html#sidebar. Line 1: status, title, unread dot, coarse time (Retry on
// hover for failed sessions, same row). Line 2 (dim, 11 px): agent · branch icon · branch · ↑ahead ↓behind ±dirty.
// Line 3, only with uncommitted lines or a PR: +added −removed · PR pill.
// Allow / Always / Deny are always visible on `perm` cards whose prompt the daemon can answer (`perm.always` is set
// once it read the prompt; otherwise the card says "Answer in the terminal"). Hover and focus never add rows. Always
// reads "Auto" when it would switch the agent to auto mode, and is hidden when the prompt has no such option.

export interface SessionCardProps {
  session: Session;
  project: Project;
  selected: boolean;
  /** Coarse clock (changes only when a visible label changes). */
  now: number;
  /** Title of the session that started this one (proto 5 `parent_session`): the card sits under it, "started by …". */
  startedBy?: string;
  /** Indent under the parent card, joined to it by a line. */
  child?: boolean;
  /** The last child of its parent: the line ends there. */
  lastChild?: boolean;
  /** Children of this session that are done / failed: the badge. */
  childrenDone?: number;
  childrenFailed?: number;
  renaming: boolean;
  /** An Allow / Always / Deny is on its way to the daemon. */
  answering: boolean;
  onSelect: (id: SessionId) => void;
  onMenu: (id: SessionId, x: number, y: number) => void;
  onResolve: (id: SessionId, decision: PermDecision) => void;
  onRetry: (id: SessionId) => void;
  onRename: (id: SessionId, title: string) => void;
  onCancelRename: () => void;
}

const ghost =
  'bg-black/5 hover:bg-black/10 dark:bg-white/10 dark:hover:bg-white/15';

const mmss = (ms: number) => {
  const t = Math.max(0, Math.floor(ms / 1000));
  return t >= 60 ? `${Math.floor(t / 60)}m ${String(t % 60).padStart(2, '0')}s` : `${t}s`;
};

/** The agent as a small glyph on the git line: the label would take about 70 px of a narrow line. */
const AGENT_GLYPH: Record<Session['agent'], LucideIcon> = { terminal: SquareTerminal, claude_code: Sparkles, codex: Bot, opencode: Bot };

function SessionCardImpl({ session: s, project, selected, now, startedBy, child, lastChild = false, childrenDone = 0, childrenFailed = 0, renaming, answering, onSelect, onMenu, onResolve, onRetry, onRename, onCancelRename }: SessionCardProps) {
  const title = s.title || AGENT_LABEL[s.agent];
  const agent = AGENT_LABEL[s.agent];
  const age = sessionAge(s, now);
  const isGit = project.is_git && s.git.is_git;
  const hasDiffstat = (s.git.added ?? 0) > 0 || (s.git.removed ?? 0) > 0;
  const needsSub = s.sub_agents.some((x) => x.status === 'perm' || x.status === 'fail');
  const showSubs = selected || needsSub;
  const running = s.sub_agents.filter((x) => x.status === 'run').length;
  const hasTodo = (s.todo?.total ?? 0) > 0;
  const compactSoon = s.context_pct !== undefined && s.context_pct >= COMPACT_AT;
  const sourceTitle = s.source_layer ? SOURCE_TITLE[s.source_layer] : undefined;
  const fail = s.status === 'fail';
  const noSignal = noLiveStatus(s, now);
  // Fixed-size inline hint on line 2 (no new row, no layout shift when it appears).
  const hint = noSignal ? (
    <>
      <span className="mx-0.5 shrink-0 opacity-40">·</span>
      <span
        data-part="no-signal"
        className="min-w-0 truncate italic"
        title="No hook or status signal has arrived from this agent yet. Check that its Lotse integration is installed."
      >
        No live status yet
      </span>
    </>
  ) : null;
  const AgentGlyph = AGENT_GLYPH[s.agent];
  const diffstat = hasDiffstat && (
    <span data-part="diffstat" className="shrink-0 tabular-nums" title="Lines added and removed by the uncommitted changes">
      <span className="text-green">+{s.git.added ?? 0}</span> <span className="text-red">−{s.git.removed ?? 0}</span>
    </span>
  );
  const context = s.context_pct !== undefined && (
    <span
      data-part="context"
      className={`inline-flex shrink-0 items-center gap-1 tabular-nums ${compactSoon ? 'text-orange' : ''}`}
      title={`Context window ${clampPct(s.context_pct)}% used${compactSoon ? ': the agent compacts it soon' : ''}`}
    >
      <span aria-hidden="true" className="h-1 w-6 overflow-hidden rounded-full bg-line">
        <span className={`block h-full ${compactSoon ? 'bg-orange' : 'bg-dim'}`} style={{ width: `${clampPct(s.context_pct)}%` }} />
      </span>
      <span className="sr-only">ctx </span>
      {clampPct(s.context_pct)}%
    </span>
  );
  // A tree of one-line rows: the details (agent, branch, changes, context) open on the selected session and on one
  // that needs you; the others carry them in the tooltip.
  const expanded = selected || s.status === 'perm' || s.status === 'fail' || answering || noSignal;
  const detailTitle = [
    title,
    [agent, isGit ? shortBranch(s.git.branch || 'HEAD') : ''].filter(Boolean).join(' · '),
    hasDiffstat ? `+${s.git.added ?? 0} −${s.git.removed ?? 0}` : '',
    s.context_pct !== undefined ? `ctx ${clampPct(s.context_pct)}%` : '',
  ]
    .filter(Boolean)
    .join('\n');
  const timeTitle =
    s.status === 'run' && s.running_since !== null
      ? `Running ${mmss(now - s.running_since)} · details in Usage & time (⇧⌘U)`
      : `Last activity ${age} · details in Usage & time (⇧⌘U)`;

  const decide = (decision: PermDecision) => (e: ReactMouseEvent) => {
    e.stopPropagation();
    onResolve(s.id, decision);
  };

  return (
    <div
      data-sid={s.id}
      data-session-id={s.id}
      draggable
      aria-current={selected ? 'true' : undefined}
      className={`group @container relative ${child ? 'ml-8' : 'ml-3'} mr-1 cursor-pointer rounded-md px-2 py-1.5 text-left has-focus-visible:ring-2 has-focus-visible:ring-blue ${
        selected ? 'bg-sel' : 'hover:bg-black/5 dark:hover:bg-white/5'
      }`}
      onClick={() => onSelect(s.id)}
      onContextMenu={(e) => {
        e.preventDefault();
        e.stopPropagation();
        onMenu(s.id, e.clientX, e.clientY);
      }}
      onDragStart={(e) => {
        e.dataTransfer.setData('text/lotse-session', s.id);
        e.dataTransfer.effectAllowed = 'copy';
      }}
    >
      {child && (
        <>
          {/* the elbow from the parent's line to this row, and the line down to the next sibling */}
          <span aria-hidden="true" className="pointer-events-none absolute top-[-3px] -left-[5px] h-5 w-[11px] rounded-bl-md border-b border-l border-line" />
          {!lastChild && <span aria-hidden="true" className="pointer-events-none absolute top-[17px] -bottom-0.5 -left-[5px] border-l border-line" />}
        </>
      )}
      <div className="flex items-center gap-2">
        <StatusGlyph status={displayStatus(s)} label={noSignal ? 'Detecting… no live status yet' : undefined} />
        {renaming ? (
          <InlineEdit value={s.title} label="Session title" onCommit={(t) => onRename(s.id, t)} onCancel={onCancelRename} />
        ) : (
          <button
            type="button"
            data-card-main
            className="min-w-0 flex-1 cursor-pointer truncate text-left outline-none"
            title={expanded ? title : detailTitle}
            aria-current={selected ? 'true' : undefined}
          >
            {title}
          </button>
        )}
        {s.unread && !selected && <span className="h-2 w-2 shrink-0 rounded-full bg-blue" role="img" aria-label="Unread" />}
        <span
          className={`shrink-0 text-[11px] text-dim tabular-nums ${fail ? 'group-focus-within:hidden group-hover:hidden' : ''}`}
          title={timeTitle}
        >
          {age}
        </span>
        {fail && (
          <button
            type="button"
            className={`-my-0.5 hidden h-[18px] shrink-0 items-center gap-1 rounded px-1.5 text-[11px] group-focus-within:inline-flex group-hover:inline-flex ${ghost}`}
            title="Retry (opens the session's last command)"
            onClick={(e) => {
              e.stopPropagation();
              onRetry(s.id);
            }}
          >
            <RotateCcw size={11} strokeWidth={2} aria-hidden="true" />
            Retry
          </button>
        )}
      </div>

      {expanded && (isGit ? (
        <div
          data-line="git"
          className="flex min-w-0 flex-wrap items-center gap-x-1.5 gap-y-0 pl-[22px] text-[11px] text-dim/80"
          title={s.git.worktree ? `Isolated worktree · ${s.git.worktree_path ?? ''}` : 'Main checkout'}
        >
          <span data-part="agent" className="inline-flex shrink-0 items-center whitespace-nowrap" title={sourceTitle}>
            <AgentGlyph size={11} strokeWidth={2} aria-hidden="true" />
            <span className="sr-only">{agent}</span>
          </span>
          {/* In a very narrow sidebar the branch icon yields first, so the agent and the counts stay whole. */}
          {s.git.worktree ? (
            <FolderGit2 size={11} strokeWidth={2} className="flex-none @max-[170px]:hidden" aria-hidden="true" />
          ) : (
            <GitBranch size={11} strokeWidth={2} className="flex-none @max-[170px]:hidden" aria-hidden="true" />
          )}
          <span data-part="branch" className="min-w-0 truncate font-mono" title={s.git.branch}>
            {shortBranch(s.git.branch || 'HEAD')}
          </span>
          {s.git.ahead > 0 && (
            <span data-part="ahead" className="shrink-0 tabular-nums" title={`${s.git.ahead} commit${s.git.ahead === 1 ? '' : 's'} ahead`}>
              ↑{s.git.ahead}
            </span>
          )}
          {(s.git.behind ?? 0) > 0 && (
            <span data-part="behind" className="shrink-0 tabular-nums" title={`${s.git.behind} commit${s.git.behind === 1 ? '' : 's'} behind the base`}>
              ↓{s.git.behind}
            </span>
          )}
          {s.git.dirty > 0 && (
            <span data-part="dirty" className="shrink-0 tabular-nums" title={`${s.git.dirty} uncommitted change${s.git.dirty === 1 ? '' : 's'}`}>
              ±{s.git.dirty}
            </span>
          )}
          {/* Changes and context share this line while they fit; in a narrow sidebar they wrap below the branch. */}
          {diffstat}
          {context}
          {hint}
        </div>
      ) : (
        <div className="truncate pl-[22px] text-[11px] text-dim/80">
          <span data-part="agent" title={sourceTitle}>
            {agent}
          </span>
          {hint}
        </div>
      ))}

      {startedBy !== undefined && expanded && (
        <div data-part="started-by" className="truncate pl-[22px] text-[11px] text-dim/80" title={`Started by ${startedBy}`}>
          started by {startedBy}
        </div>
      )}
      {expanded && (childrenDone > 0 || childrenFailed > 0) && (
        <div data-part="child-badge" className="flex min-w-0 items-center gap-2 truncate pl-[22px] text-[11px]">
          {childrenFailed > 0 && (
            <span className="text-red" title="A child session failed">
              ✕ {childrenFailed} child{childrenFailed === 1 ? '' : 'ren'} failed
            </span>
          )}
          {childrenDone > 0 && (
            <span className="text-green" title="A child session is done">
              ✓ {childrenDone} child{childrenDone === 1 ? '' : 'ren'} done
            </span>
          )}
        </div>
      )}

      {/* The pull request badge: its own row. */}
      {expanded && isGit && s.git.pr && (
        <div data-line="diff" className="flex min-w-0 items-center gap-1.5 pl-[22px] text-[11px] text-dim/80">
          {s.git.pr && (
            <span data-part="pr" className="min-w-0 truncate rounded bg-black/5 px-1 text-[10px] dark:bg-white/10" title={prTitle(s.git.pr)}>
              {prLabel(s.git.pr)}
            </span>
          )}
        </div>
      )}

      {/* Mod data (proto 5): the todo list, and the context share of a session with no git line. */}
      {expanded && ((!isGit && s.context_pct !== undefined) || hasTodo) && (
        <div data-line="mod" className="flex min-w-0 items-center gap-2 pl-[22px] text-[11px] text-dim/80">
          {!isGit && context}
          {hasTodo && s.todo && (
            <span data-part="todo" className="flex min-w-0 items-center gap-1 tabular-nums" title={`${s.todo.done} of ${s.todo.total} todos done`}>
              <span
                role="progressbar"
                aria-label="Todo"
                aria-valuemin={0}
                aria-valuemax={s.todo.total}
                aria-valuenow={s.todo.done}
                className="h-1 w-10 shrink-0 overflow-hidden rounded-full bg-line"
              >
                <span className="block h-full bg-blue" style={{ width: `${Math.min(100, (s.todo.done / s.todo.total) * 100)}%` }} />
              </span>
              {s.todo.done}/{s.todo.total}
            </span>
          )}
        </div>
      )}

      {/* No `perm.always`: the daemon has not read an answerable prompt off the screen (a folder-trust dialog, or the
          prompt is still being drawn), and an answer would fail with "not waiting at a permission prompt that can be
          answered". So no buttons: the one place to answer it is the terminal. */}
      {s.status === 'perm' && s.perm?.always === undefined && (
        <div
          className="mt-1.5 flex h-[22px] items-center pl-[22px] text-[11px] italic text-dim/80"
          role="group"
          aria-label={`Permission: ${title}`}
          title={s.perm ? `${s.perm.tool}: ${s.perm.summary}` : undefined}
        >
          Answer in the terminal
        </div>
      )}
      {s.status === 'perm' && s.perm?.always !== undefined && (
        <div
          className="mt-1.5 flex flex-wrap gap-1 pl-[22px]"
          role="group"
          aria-label={`Permission: ${title}`}
          aria-busy={answering}
          title={s.perm ? `${s.perm.tool}: ${s.perm.summary}` : undefined}
        >
          <button
            type="button"
            className="h-[22px] rounded-md bg-blue px-2.5 text-[11px] font-medium text-white transition-colors hover:brightness-110 disabled:opacity-60 disabled:hover:brightness-100"
            disabled={answering}
            onClick={decide('allow')}
          >
            Allow
          </button>
          {s.perm?.always !== 'unavailable' && (
            <button
              type="button"
              className={`h-[22px] rounded-md px-2.5 text-[11px] font-medium transition-colors disabled:opacity-60 ${ghost}`}
              disabled={answering}
              onClick={decide('always')}
              title={s.perm?.always === 'auto_mode' ? 'Yes, and switch to auto mode: this session stops asking' : undefined}
            >
              {s.perm?.always === 'auto_mode' ? 'Auto' : 'Always'}
            </button>
          )}
          <button
            type="button"
            className={`h-[22px] rounded-md px-2.5 text-[11px] font-medium transition-colors disabled:opacity-60 ${ghost}`}
            disabled={answering}
            onClick={decide('deny')}
          >
            Deny
          </button>
        </div>
      )}

      {!showSubs && s.sub_agents.length > 0 && (
        <div className="pl-[22px] text-[11px] text-dim/80">
          {s.sub_agents.length} sub-agent{s.sub_agents.length > 1 ? 's' : ''}
          {running > 0 ? ` · ${running} running` : ''}
        </div>
      )}
      {showSubs &&
        orderSubAgents(s.sub_agents).map(({ agent: x, depth }) => (
          <div key={x.id} data-sub-agent={x.id} data-depth={depth} className="mt-0.5 text-[12px] text-dim" style={{ paddingLeft: 22 + depth * 12 }}>
            <div className="flex items-center gap-2">
              <StatusGlyph status={x.status} size={12} />
              <span className="min-w-0 truncate" title={x.name}>
                {x.name}
              </span>
              {x.background && (
                <span data-part="background" className="shrink-0 rounded bg-black/5 px-1 text-[10px] dark:bg-white/10" title="Runs in the background">
                  bg
                </span>
              )}
              {x.tokens !== undefined && (
                <span
                  data-part="usage"
                  className="ml-auto shrink-0 text-[10px] tabular-nums"
                  title={[x.model, `${x.tokens.toLocaleString('en-US')} tokens`, x.cost_usd !== undefined && formatCost(x.cost_usd)].filter(Boolean).join(' · ')}
                >
                  {formatTokens(x.tokens)}
                  {x.cost_usd !== undefined && ` · ${formatCost(x.cost_usd)}`}
                </span>
              )}
            </div>
            {x.activity && (
              <div data-part="activity" className="truncate pl-5 text-[11px] opacity-80" title={x.activity}>
                {x.activity}
              </div>
            )}
          </div>
        ))}
    </div>
  );
}

export const SessionCard = memo(SessionCardImpl);
