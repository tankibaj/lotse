import { FolderGit2, GitBranch } from 'lucide-react';
import { useClient } from '../lib/client/context';
import {
  formatUsage,
  needYouCount,
  toggleNeedFilter,
  togglePausedNotifications,
  useSessionShortcuts,
  useUiState,
  useWorkspace,
} from '../lib/state';
import { useSelectedSessionId } from '../lib/ui/selection';
import { lanState } from '../settings/remote';
import { NoGitIcon } from '../sidebar/NoGit';

// Alpha status bar (spec/mock.html footer): need-you filter chip, git of the current session, daemon
// state and session count, pause-notifications bell, LAN state (`settings/remote`), today's tokens and cost (`—` until the first snapshot).

const chip = 'rounded px-1.5 hover:bg-black/5 dark:hover:bg-white/10';

const Bell = ({ off }: { off: boolean }) => (
  <svg width="13" height="13" fill="none" stroke="currentColor" strokeWidth="1.4" className="inline align-[-2px]" aria-hidden="true">
    <path d="M3.5 10V6.5a3 3 0 0 1 6 0V10l1 1.2h-8z" />
    <path d="M5.5 12.2a1 1 0 0 0 2 0" />
    {off && <path d="M2 2l10 10" />}
  </svg>
);

export function StatusBar() {
  const client = useClient();
  const { projects, sessions, settings, connection, usage: usageToday } = useWorkspace();
  const usage = formatUsage(usageToday);
  const { needFilter } = useUiState();
  const selectedId = useSelectedSessionId();
  useSessionShortcuts();

  const need = needYouCount(sessions);
  const filterOn = needFilter && need > 0;
  const current = sessions.find((s) => s.id === selectedId);
  const project = current && projects.find((p) => p.id === current.project_id);
  const git = current?.git;
  const paused = settings?.notifications.paused ?? false;
  const up = connection === 'connected';
  const lan = lanState();

  return (
    <footer className="flex h-7 shrink-0 items-center gap-4 border-t bg-side lotse-glass px-3 text-[11px] text-dim">
      <button
        type="button"
        aria-pressed={filterOn}
        disabled={need === 0 && !filterOn}
        title="Show only what needs you"
        onClick={() => toggleNeedFilter(sessions)}
        className={`-mx-1.5 font-medium ${chip} ${need > 0 ? 'text-orange' : 'text-dim'}`}
      >
        {filterOn ? `▲ Showing ${need} that need you · Show all` : `▲ ${need} need you`}
      </button>

      {current && project && git && (
        <span className="flex min-w-0 items-center gap-1.5" data-testid="status-git">
          {project.is_git && git.is_git ? (
            <>
              <span
                className="inline-flex min-w-0 items-center gap-1"
                title={git.worktree ? `Isolated worktree · ${git.worktree_path ?? ''}` : 'Main checkout'}
              >
                {git.worktree ? <FolderGit2 size={12} strokeWidth={2} aria-hidden="true" /> : <GitBranch size={12} strokeWidth={2} aria-hidden="true" />}
                <span className="max-w-[220px] truncate font-mono" title={git.branch}>
                  {git.branch || 'HEAD'}
                </span>
              </span>
              {git.worktree && <span className="rounded bg-line px-1 text-[10px]">worktree</span>}
              {git.ahead > 0 && (
                <span className="tabular-nums" title={`${git.ahead} commit${git.ahead === 1 ? '' : 's'} ahead`}>
                  ↑{git.ahead}
                </span>
              )}
              {git.dirty > 0 && (
                <span className="tabular-nums" title={`${git.dirty} uncommitted change${git.dirty === 1 ? '' : 's'}`}>
                  ±{git.dirty}
                </span>
              )}
            </>
          ) : (
            <span className="inline-flex items-center gap-1" title={`${project.name} is not a git repository`}>
              <NoGitIcon size={12} />
              No git
            </span>
          )}
          <span className="opacity-40">·</span>
        </span>
      )}

      <span className={up ? '' : 'text-red'}>
        ● {up ? 'lotsed running' : 'lotsed not running'} · {sessions.length} session{sessions.length === 1 ? '' : 's'}
      </span>

      <button
        type="button"
        aria-pressed={paused}
        aria-label="Pause notifications"
        title="Pause notifications ⌥⌘N"
        onClick={() => void togglePausedNotifications(client).catch(() => {})}
        className={`ml-auto ${chip}`}
      >
        <Bell off={paused} />
        {paused && ' Paused'}
      </button>
      <span title={lan.tooltip}>{lan.label}</span>
      <span data-testid="status-usage" title={usage.title}>
        {usage.text}
      </span>
    </footer>
  );
}
