import { memo } from 'react';
import type { Session, Workspace } from '../lib/client/types';
import { rollupSummary } from '../lib/state/selectors';
import { InlineEdit } from './InlineEdit';
import { Rollup } from './Rollup';

// Workspace header: shown only when there is more than one workspace. Collapsible like a project row;
// collapsed, it carries the roll-up of everything below it (the most urgent state wins).

export interface WorkspaceRowProps {
  workspace: Workspace;
  sessions: Session[];
  /** Projects in it (empty workspaces show "No projects" when open). */
  projectCount: number;
  open: boolean;
  renaming: boolean;
  onToggle: (id: string) => void;
  onMenu: (id: string, x: number, y: number) => void;
  onRename: (id: string, name: string) => void;
  onCancelRename: () => void;
}

const Chevron = ({ open }: { open: boolean }) => (
  <svg
    className="h-3 w-3 shrink-0 opacity-60"
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

function WorkspaceRowImpl({ workspace: w, sessions, projectCount, open, renaming, onToggle, onMenu, onRename, onCancelRename }: WorkspaceRowProps) {
  if (renaming) {
    return (
      <div className="flex w-full items-center gap-1.5 py-1 pr-2 pl-1">
        <Chevron open={open} />
        <InlineEdit value={w.name} label="Workspace name" onCommit={(n) => onRename(w.id, n)} onCancel={onCancelRename} />
      </div>
    );
  }
  return (
    <button
      type="button"
      data-wid={w.id}
      aria-expanded={open}
      title={`${w.name} · ${projectCount} project${projectCount === 1 ? '' : 's'}`}
      className="flex w-full items-center gap-1.5 rounded-md py-1 pr-2 pl-1 text-left text-[11px] font-semibold tracking-wide text-dim uppercase hover:bg-black/5 dark:hover:bg-white/5"
      onClick={() => onToggle(w.id)}
      onContextMenu={(e) => {
        e.preventDefault();
        onMenu(w.id, e.clientX, e.clientY);
      }}
    >
      <Chevron open={open} />
      <span className="min-w-0 flex-1 truncate">{w.name}</span>
      {!open && <Rollup sum={rollupSummary(sessions)} />}
    </button>
  );
}

export const WorkspaceRow = memo(WorkspaceRowImpl);
