import { Bot, EyeOff, PanelLeftClose, Sparkles, SquareTerminal, X } from 'lucide-react';
import type { LucideIcon } from 'lucide-react';
import type { Project, Session } from '../lib/client';
import { shortBranch } from '../lib/state';
import { ProjectTile } from './ProjectTile';
import { StatusGlyph } from './StatusGlyph';

export const AGENT_LABEL: Record<Session['agent'], string> = {
  terminal: 'Terminal',
  claude_code: 'Claude Code',
  codex: 'Codex',
  opencode: 'OpenCode',
};
const AGENT_ICON: Record<Session['agent'], LucideIcon> = { terminal: SquareTerminal, claude_code: Sparkles, codex: Bot, opencode: Bot };

/** The session's branch with ↑ahead ↓behind ±dirty (zero counts left out); nothing for a folder that is not a git repository. */
function BranchInfo({ git }: { git: Session['git'] }) {
  if (!git.is_git || git.branch === '') return null;
  const counts = [git.ahead > 0 && `↑${git.ahead}`, (git.behind ?? 0) > 0 && `↓${git.behind}`, git.dirty > 0 && `±${git.dirty}`].filter(Boolean);
  return (
    <span className="min-w-0 truncate text-[11px] text-tdim" title={git.branch}>
      {shortBranch(git.branch)}
      {counts.length > 0 && <span className="tabular-nums"> {counts.join(' ')}</span>}
    </span>
  );
}

const stop = { onMouseDown: (e: { stopPropagation: () => void }) => e.stopPropagation() };

function BarButton({ label, title, onClick, children }: { label: string; title: string; onClick: () => void; children: React.ReactNode }) {
  return (
    <button
      className="grid h-5 w-5 shrink-0 place-items-center rounded-md text-dim hover:bg-sel"
      aria-label={label}
      title={title}
      {...stop}
      onClick={(e) => {
        e.stopPropagation();
        onClick();
      }}
    >
      {children}
    </button>
  );
}

interface Props {
  session: Session;
  project: Project | undefined;
  focused: boolean;
  /** Omitted for the last pane: it stays open. */
  onClose?: () => void;
  /** Omitted while this is the only open column. */
  onCollapse?: () => void;
  onHide?: () => void;
}

/** The status bar on top of a terminal pane (spec/mock.html pane header): agent, project tile, title · project, status, branch; the focused pane has the blue ring. */
export function PaneBar({ session: s, project, focused, onClose, onCollapse, onHide }: Props) {
  const Agent = AGENT_ICON[s.agent];
  return (
    <div
      className="flex h-7 shrink-0 items-center gap-2 border-b px-3 text-[12px] group-data-[topbar-hidden]/panes:hidden"
      style={focused ? { boxShadow: 'inset 0 2px 0 var(--blue)' } : undefined}
      data-testid="pane-status"
      data-focused={focused}
    >
      <span role="img" aria-label={AGENT_LABEL[s.agent]} title={AGENT_LABEL[s.agent]} className="inline-grid shrink-0 text-dim">
        <Agent size={13} aria-hidden="true" />
      </span>
      {project && <ProjectTile project={project} />}
      <span className="min-w-0 truncate">
        {s.title} <span className="text-tdim">· {project?.name}</span>
      </span>
      <StatusGlyph status={s.status} />
      <BranchInfo git={s.git} />
      <span className="flex-1" />
      {onCollapse && (
        <BarButton label="Collapse to rail" title="Collapse column to a rail" onClick={onCollapse}>
          <PanelLeftClose size={12} aria-hidden="true" />
        </BarButton>
      )}
      {onHide && (
        <BarButton label="Hide column" title="Hide column (select its session to bring it back)" onClick={onHide}>
          <EyeOff size={12} aria-hidden="true" />
        </BarButton>
      )}
      {onClose && (
        <BarButton label="Close pane" title="Close pane ⌘W" onClick={onClose}>
          <X size={12} aria-hidden="true" />
        </BarButton>
      )}
    </div>
  );
}
