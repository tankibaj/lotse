import { BellOff, GitBranch } from 'lucide-react';
import { memo } from 'react';
import type { Project, Session } from '../lib/client/types';
import { rollupSummary, shortBranch } from '../lib/state/selectors';
import { InlineEdit } from './InlineEdit';
import { NoGitIcon } from './NoGit';
import { ProjectTile } from './ProjectTile';
import { Rollup } from './Rollup';

// Project row: chevron (only on hover or focus, Finder style), coloured tile, name, no-git and muted
// badges, and, when collapsed, the roll-up (▲2 ✕1, else the top state; idle shows nothing).

export interface ProjectRowProps {
  project: Project;
  sessions: Session[];
  open: boolean;
  /** The main checkout's branch and changes, shown once on the open row instead of on every session. */
  main?: { branch: string; dirty: number; added: number; removed: number } | null;
  renaming: boolean;
  onToggle: (id: string) => void;
  onMenu: (id: string, x: number, y: number) => void;
  onRename: (id: string, name: string) => void;
  onCancelRename: () => void;
}

const NoGit = () => (
  <span className="flex-none text-dim" title="Not a git repository" role="img" aria-label="Not a git repository">
    <NoGitIcon />
  </span>
);

const Chevron = ({ open }: { open: boolean }) => (
  <svg
    className="h-3 w-3 shrink-0 text-dim opacity-60 transition-transform group-hover/p:opacity-100 group-focus-visible/p:opacity-100"
    style={{ transform: `rotate(${open ? 90 : 0}deg)` }}
    viewBox="0 0 12 12"
    fill="none"
    stroke="currentColor"
    strokeWidth="1.6"
    aria-hidden="true"
  >
    <path d="M4.5 2.5 8 6l-3.5 3.5" />
  </svg>
);

function ProjectRowImpl({ project: p, sessions, open, main, renaming, onToggle, onMenu, onRename, onCancelRename }: ProjectRowProps) {
  const sum = open ? null : rollupSummary(sessions);
  const mode = p.notifications.mode;

  if (renaming) {
    return (
      <div className="flex w-full items-center gap-1.5 py-1 pr-2 pl-1">
        <ProjectTile color={p.color} icon={p.icon} />
        <InlineEdit value={p.name} label="Project name" onCommit={(n) => onRename(p.id, n)} onCancel={onCancelRename} />
      </div>
    );
  }

  return (
    <button
      type="button"
      data-pid={p.id}
      aria-expanded={open}
      className="group/p flex w-full items-center gap-1.5 rounded-md py-1.5 pr-2 pl-1 text-left hover:bg-black/5 dark:hover:bg-white/5"
      onClick={() => onToggle(p.id)}
      onContextMenu={(e) => {
        e.preventDefault();
        onMenu(p.id, e.clientX, e.clientY);
      }}
    >
      <ProjectTile color={p.color} icon={p.icon} />
      <span className="ml-0.5 min-w-0 truncate text-[12px] font-semibold text-dim" title={p.name}>
        {p.name}
      </span>
      <Chevron open={open} />
      <span className="min-w-0 flex-1" />
      {!p.is_git && <NoGit />}
      {mode === 'mute' && (
        <span className="flex-none text-dim" title="Notifications muted" role="img" aria-label="Notifications muted">
          <BellOff size={12} strokeWidth={1.5} aria-hidden="true" />
        </span>
      )}
      {mode === 'custom' && (
        <span className="text-[10px] text-dim" title="Custom notifications" role="img" aria-label="Custom notifications">
          ●
        </span>
      )}
      {open && main && (
        <span
          data-part="main-checkout"
          className="flex max-w-[50%] min-w-0 shrink items-center gap-1 text-[11px] font-normal text-dim/80"
          title={`Main checkout: ${main.branch}${main.dirty ? ` · ${main.dirty} uncommitted` : ''}`}
        >
          <GitBranch size={11} strokeWidth={2} className="flex-none" aria-hidden="true" />
          <span className="min-w-0 truncate font-mono">{shortBranch(main.branch)}</span>
          {main.dirty > 0 && <span className="shrink-0 tabular-nums">±{main.dirty}</span>}
          {(main.added > 0 || main.removed > 0) && (
            <span className="shrink-0 tabular-nums">
              <span className="text-green">+{main.added}</span> <span className="text-red">−{main.removed}</span>
            </span>
          )}
        </span>
      )}
      {sum && <Rollup sum={sum} />}
    </button>
  );
}

export const ProjectRow = memo(ProjectRowImpl);
