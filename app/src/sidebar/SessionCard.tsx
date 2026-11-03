import { FolderGit2, GitBranch, RotateCcw } from 'lucide-react';
import { memo } from 'react';
import type { MouseEvent as ReactMouseEvent } from 'react';
import type { PermDecision, Project, Session, SessionId } from '../lib/client/types';
import { AGENT_LABEL, displayStatus, noLiveStatus, sessionAge, shortBranch } from '../lib/state/selectors';
import { InlineEdit } from './InlineEdit';
import { StatusGlyph } from './StatusGlyph';

// Session card, spec/research.html#sidebar. Line 1: status, title, unread dot, coarse time (Retry on
// hover for failed sessions, same row). Line 2 (dim, 11 px): agent · branch icon · branch · ↑ahead · ±dirty.
// Allow / Always / Deny are always visible on `perm` cards whose prompt the daemon can answer (`perm.always` is set
// once it read the prompt; otherwise the card says "Answer in the terminal"). Hover and focus never add rows. Always
// reads "Auto" when it would switch the agent to auto mode, and is hidden when the prompt has no such option.

export interface SessionCardProps {
  session: Session;
  project: Project;
  selected: boolean;
  /** Coarse clock (changes only when a visible label changes). */
  now: number;
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

function SessionCardImpl({ session: s, project, selected, now, renaming, answering, onSelect, onMenu, onResolve, onRetry, onRename, onCancelRename }: SessionCardProps) {
  const title = s.title || AGENT_LABEL[s.agent];
  const agent = AGENT_LABEL[s.agent];
  const age = sessionAge(s, now);
  const isGit = project.is_git && s.git.is_git;
  const needsSub = s.sub_agents.some((x) => x.status === 'perm' || x.status === 'fail');
  const showSubs = selected || needsSub;
  const running = s.sub_agents.filter((x) => x.status === 'run').length;
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
      className={`group @container ml-4 mr-1 cursor-pointer rounded-md px-2 py-1.5 text-left has-focus-visible:ring-2 has-focus-visible:ring-blue ${
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
      <div className="flex items-center gap-2">
        <StatusGlyph status={displayStatus(s)} label={noSignal ? 'Detecting… no live status yet' : undefined} />
        {renaming ? (
          <InlineEdit value={s.title} label="Session title" onCommit={(t) => onRename(s.id, t)} onCancel={onCancelRename} />
        ) : (
          <button
            type="button"
            data-card-main
            className="min-w-0 flex-1 cursor-pointer truncate text-left outline-none"
            title={title}
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

      {isGit ? (
        <div
          data-line="git"
          className="flex min-w-0 items-center gap-1 pl-[22px] text-[11px] text-dim"
          title={s.git.worktree ? `Isolated worktree · ${s.git.worktree_path ?? ''}` : 'Main checkout'}
        >
          <span data-part="agent" className="shrink-0 whitespace-nowrap">
            {agent}
          </span>
          <span className="mx-0.5 shrink-0 opacity-40 @max-[170px]:mx-0">·</span>
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
          {s.git.dirty > 0 && (
            <span data-part="dirty" className="shrink-0 tabular-nums" title={`${s.git.dirty} uncommitted change${s.git.dirty === 1 ? '' : 's'}`}>
              ±{s.git.dirty}
            </span>
          )}
          {hint}
        </div>
      ) : (
        <div className="truncate pl-[22px] text-[11px] text-dim">
          {agent}
          {hint}
        </div>
      )}

      {/* No `perm.always`: the daemon has not read an answerable prompt off the screen (a folder-trust dialog, or the
          prompt is still being drawn), and an answer would fail with "not waiting at a permission prompt that can be
          answered". So no buttons: the one place to answer it is the terminal. */}
      {s.status === 'perm' && s.perm?.always === undefined && (
        <div
          className="mt-1.5 flex h-[22px] items-center pl-[22px] text-[11px] italic text-dim"
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
        <div className="pl-[22px] text-[11px] text-dim">
          {s.sub_agents.length} sub-agent{s.sub_agents.length > 1 ? 's' : ''}
          {running > 0 ? ` · ${running} running` : ''}
        </div>
      )}
      {showSubs &&
        s.sub_agents.map((x) => (
          <div key={x.id} data-sub-agent={x.id} className="mt-0.5 flex items-center gap-2 pl-[22px] text-[12px] text-dim">
            <StatusGlyph status={x.status} size={12} />
            <span className="min-w-0 truncate" title={x.name}>
              {x.name}
            </span>
          </div>
        ))}
    </div>
  );
}

export const SessionCard = memo(SessionCardImpl);
