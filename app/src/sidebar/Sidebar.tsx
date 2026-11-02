import { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import type { KeyboardEvent as ReactKeyboardEvent } from 'react';
import { useClient } from '../lib/client/context';
import type { PermDecision, ProjectId, SessionId } from '../lib/client/types';
import {
  filterGroups,
  groupSessions,
  needYouCount,
  timeSignature,
  toggleCollapsed,
  toggleNeedFilter,
  useCoarseNow,
  useUiState,
  useWorkspace,
  visibleSessionIds,
} from '../lib/state';
import { emitUiCommand } from '../lib/ui/commands';
import { hostBridge } from '../sheets/hostBridge';
import { selectSession, useSelectedSessionId } from '../lib/ui/selection';
import { ConfirmDialog } from './ConfirmDialog';
import { ContextMenu } from './ContextMenu';
import { projectMenu, sessionMenu } from './menus';
import { ProjectRow } from './ProjectRow';
import { SessionCard } from './SessionCard';

type Menu = { kind: 'project' | 'session'; id: string; x: number; y: number };
type Target = { kind: 'project' | 'session'; id: string };

const noop = () => {};

/** Search, Projects header (both fixed) and the project tree (the only part that scrolls). */
export function Sidebar() {
  const client = useClient();
  const { projects, sessions, settings } = useWorkspace();
  const { needFilter, collapsed } = useUiState();
  const selectedId = useSelectedSessionId();

  const navRef = useRef<HTMLElement>(null);
  const hovering = useRef(false);
  const [menu, setMenu] = useState<Menu | null>(null);
  const [renaming, setRenaming] = useState<Target | null>(null);
  const [confirm, setConfirm] = useState<Target | null>(null);
  const [notice, setNotice] = useState<string | null>(null);
  // Sessions whose Allow / Always / Deny is on its way to the daemon: the buttons stay off meanwhile.
  const [answering, setAnswering] = useState<ReadonlySet<SessionId>>(new Set());

  // Time labels are coarse and only change the sidebar when their text changes, never under the pointer or focus.
  const signature = useCallback((n: number) => timeSignature(sessions, n), [sessions]);
  const hold = useCallback(() => hovering.current || (navRef.current?.contains(document.activeElement) ?? false), []);
  const now = useCoarseNow(signature, hold);

  const needCount = needYouCount(sessions);
  const filterOn = needFilter && needCount > 0;
  const groups = useMemo(() => groupSessions(projects, sessions), [projects, sessions]);
  const shown = useMemo(() => filterGroups(groups, filterOn), [groups, filterOn]);
  const projectById = useMemo(() => new Map(projects.map((p) => [p.id, p])), [projects]);

  const onSelect = useCallback((id: SessionId) => selectSession(id), []);
  const onToggle = useCallback((id: ProjectId) => toggleCollapsed(id), []);
  const onSessionMenu = useCallback((id: SessionId, x: number, y: number) => setMenu({ kind: 'session', id, x, y }), []);
  const onProjectMenu = useCallback((id: ProjectId, x: number, y: number) => setMenu({ kind: 'project', id, x, y }), []);
  // Keyboard users keep their place: after an inline action the control they used disappears, so focus
  // goes back to the card (or project row) it belonged to.
  const refocus = useCallback((t: Target) => {
    setTimeout(() => {
      const sel = t.kind === 'session' ? `[data-sid="${t.id}"] [data-card-main]` : `[data-pid="${t.id}"]`;
      navRef.current?.querySelector<HTMLElement>(sel)?.focus();
    }, 0);
  }, []);
  const onResolve = useCallback(
    (id: SessionId, decision: PermDecision) => {
      selectSession(id); // the terminal gets focus, so the answer is seen landing
      setAnswering((s) => new Set(s).add(id));
      // The daemon types the answer into the pane. It refuses (conflict) when no prompt is on screen
      // or the prompt has no such option: say so instead of looking like a dead button.
      client
        .resolvePermission(id, decision)
        .catch((e: unknown) => setNotice(e instanceof Error ? e.message : 'Could not answer the prompt'))
        .finally(() =>
          setAnswering((s) => {
            const next = new Set(s);
            next.delete(id);
            return next;
          }),
        );
      refocus({ kind: 'session', id });
    },
    [client, refocus],
  );
  const onRetry = useCallback(
    (id: SessionId) => {
      selectSession(id);
      // A process that still runs is not re-run: the daemon says so and the card shows it for a moment.
      client.retrySession(id).catch((e: unknown) => setNotice(e instanceof Error ? e.message : 'Retry failed'));
      refocus({ kind: 'session', id });
    },
    [client, refocus],
  );
  useEffect(() => {
    if (!notice) return;
    const t = window.setTimeout(() => setNotice(null), 5000);
    return () => window.clearTimeout(t);
  }, [notice]);
  const renamingRef = useRef<Target | null>(null);
  useEffect(() => {
    renamingRef.current = renaming;
  }, [renaming]);
  const endRename = useCallback(() => {
    const t = renamingRef.current;
    setRenaming(null);
    if (t) refocus(t);
  }, [refocus]);
  const onRenameSession = useCallback(
    (id: SessionId, title: string) => {
      endRename();
      client.renameSession(id, title).catch(noop);
    },
    [client, endRename],
  );
  const onRenameProject = useCallback(
    (id: ProjectId, name: string) => {
      endRename();
      client.updateProject(id, { name }).catch(noop);
    },
    [client, endRename],
  );
  const cancelRename = endRename;

  const stop = (id: SessionId) => client.closeSession(id).catch(noop);

  const onKeyDown = (e: ReactKeyboardEvent<HTMLElement>) => {
    const el = e.target as HTMLElement;
    if (el instanceof HTMLInputElement) return;
    const card = el.closest<HTMLElement>('[data-sid]');
    const row = el.closest<HTMLElement>('[data-pid]');
    if (e.key === 'ArrowDown' || e.key === 'ArrowUp') {
      const ids = visibleSessionIds(shown, collapsed, filterOn);
      const down = e.key === 'ArrowDown';
      let next: SessionId | undefined;
      if (!card && row) {
        // Project row focused: step to the session right after / before the row, not relative to the selection.
        const at = shown.findIndex((g) => g.project.id === row.dataset.pid);
        const before = visibleSessionIds(shown.slice(0, Math.max(0, at)), collapsed, filterOn).length;
        next = ids[down ? before : before - 1];
      } else {
        const i = ids.indexOf(card?.dataset.sid ?? selectedId ?? '');
        next = ids[Math.min(ids.length - 1, Math.max(0, i + (down ? 1 : -1)))];
      }
      if (!next) return;
      e.preventDefault();
      selectSession(next);
      navRef.current?.querySelector<HTMLElement>(`[data-sid="${next}"] [data-card-main]`)?.focus();
    } else if (row && !filterOn && (e.key === 'ArrowRight' || e.key === 'ArrowLeft')) {
      e.preventDefault();
      toggleCollapsed(row.dataset.pid ?? '', e.key === 'ArrowLeft');
    } else if (e.key === 'ContextMenu' || (e.shiftKey && e.key === 'F10')) {
      const target = card ?? row;
      if (!target) return;
      e.preventDefault();
      const r = target.getBoundingClientRect();
      if (card?.dataset.sid) onSessionMenu(card.dataset.sid, r.left + 24, r.bottom - 4);
      else if (row?.dataset.pid) onProjectMenu(row.dataset.pid, r.left + 24, r.bottom - 4);
    }
  };

  const menuProject = menu?.kind === 'project' ? projectById.get(menu.id) : undefined;
  const menuSession = menu?.kind === 'session' ? sessions.find((s) => s.id === menu.id) : undefined;
  const confirmProject = confirm?.kind === 'project' ? projectById.get(confirm.id) : undefined;
  const confirmSession = confirm?.kind === 'session' ? sessions.find((s) => s.id === confirm.id) : undefined;

  return (
    <>
      <div className="px-3">
        <div
          onClick={() => emitUiCommand('openPalette')}
          onKeyDown={(e) => {
            if (e.key === 'Enter' || e.key === ' ') {
              e.preventDefault();
              emitUiCommand('openPalette');
            }
          }}
          className="flex cursor-text items-center gap-2 rounded-md bg-line px-2 py-1.5"
          role="button"
          tabIndex={0}
          aria-label="Search, ⌘K"
        >
          <svg width="13" height="13" fill="none" stroke="currentColor" strokeWidth="1.6" className="text-dim" aria-hidden="true">
            <circle cx="5.5" cy="5.5" r="4" />
            <path d="M9 9l3 3" />
          </svg>
          <span className="text-dim">Search</span>
          <span className="ml-auto text-[11px] text-dim">⌘K</span>
        </div>
      </div>
      <div className="mt-4 mb-1 flex items-center px-4 text-[11px] font-semibold tracking-wide text-dim uppercase">
        Projects
        <button
          className="ml-auto grid h-[26px] w-7 place-items-center rounded-md hover:bg-sel"
          onClick={() => emitUiCommand('openAddProject')}
          aria-label="Add a project"
          title="Add a project ⇧⌘O"
        >
          +
        </button>
      </div>
      <nav
        ref={navRef}
        className="min-h-0 flex-1 space-y-0.5 overflow-x-hidden overflow-y-auto overscroll-contain px-2 pb-3"
        aria-label="Projects"
        onKeyDown={onKeyDown}
        onPointerEnter={() => (hovering.current = true)}
        onPointerLeave={() => (hovering.current = false)}
      >
        {notice && (
          <div role="status" className="mx-1 mb-2 rounded-md px-2 py-1.5 text-[12px] [background:color-mix(in_srgb,var(--orange)_14%,transparent)]">
            {notice}
          </div>
        )}
        {filterOn && (
          <div className="mx-1 mb-2 flex items-center gap-2 rounded-md px-2 py-1.5 text-[12px] [background:color-mix(in_srgb,var(--orange)_14%,transparent)]">
            <span className="text-orange">▲</span>
            <span className="flex-1">Needs you ({needCount})</span>
            <button className="text-blue" onClick={() => toggleNeedFilter(sessions)}>
              Show all
            </button>
          </div>
        )}
        {shown.map(({ project, sessions: list }) => {
          const open = filterOn || !collapsed.has(project.id);
          return (
            <div key={project.id} className="space-y-0.5" data-project={project.id}>
              <ProjectRow
                project={project}
                sessions={list}
                open={open}
                renaming={renaming?.kind === 'project' && renaming.id === project.id}
                onToggle={onToggle}
                onMenu={onProjectMenu}
                onRename={onRenameProject}
                onCancelRename={cancelRename}
              />
              {open && list.length === 0 && (
                <div className="py-1 pl-[38px] text-[12px] text-dim">
                  No sessions ·{' '}
                  <button className="text-blue" onClick={() => emitUiCommand('openNewSession')}>
                    New session
                  </button>
                </div>
              )}
              {open &&
                list.map((s) => (
                  <SessionCard
                    key={s.id}
                    session={s}
                    project={project}
                    selected={s.id === selectedId}
                    now={now}
                    renaming={renaming?.kind === 'session' && renaming.id === s.id}
                    answering={answering.has(s.id)}
                    onSelect={onSelect}
                    onMenu={onSessionMenu}
                    onResolve={onResolve}
                    onRetry={onRetry}
                    onRename={onRenameSession}
                    onCancelRename={cancelRename}
                  />
                ))}
            </div>
          );
        })}
      </nav>

      {menu && menuProject && (
        <ContextMenu
          x={menu.x}
          y={menu.y}
          label={`${menuProject.name} actions`}
          entries={projectMenu(menuProject, settings, client, {
            newSession: () => emitUiCommand('openNewSession', menuProject.id),
            changeStyle: () => emitUiCommand('openProjectStyle', menuProject.id),
            reveal: hostBridge().revealInFinder
              ? () => void hostBridge().revealInFinder?.(menuProject.path)?.catch(noop)
              : undefined,
            rename: () => setRenaming({ kind: 'project', id: menuProject.id }),
            remove: () => setConfirm({ kind: 'project', id: menuProject.id }),
          })}
          onClose={() => setMenu(null)}
        />
      )}
      {menu && menuSession && (
        <ContextMenu
          x={menu.x}
          y={menu.y}
          label={`${menuSession.title} actions`}
          entries={sessionMenu(menuSession, {
            open: () => selectSession(menuSession.id),
            split: () => emitUiCommand('splitSession', menuSession.id),
            splitDown: () => emitUiCommand('splitSessionDown', menuSession.id),
            rename: () => setRenaming({ kind: 'session', id: menuSession.id }),
            stop: () =>
              menuSession.status === 'run' || menuSession.status === 'perm'
                ? setConfirm({ kind: 'session', id: menuSession.id })
                : stop(menuSession.id),
          })}
          onClose={() => setMenu(null)}
        />
      )}
      {confirmProject && (
        <ConfirmDialog
          title={`Remove ${confirmProject.name} from Lotse?`}
          body="Its sessions are stopped. Files on disk are not touched."
          confirmLabel="Remove"
          onCancel={() => setConfirm(null)}
          onConfirm={() => {
            setConfirm(null);
            client.removeProject(confirmProject.id).catch(noop);
          }}
        />
      )}
      {confirmSession && (
        <ConfirmDialog
          title={`Stop ${confirmSession.title}?`}
          body="The session is still working. Stopping ends its terminal and any running command."
          confirmLabel="Stop"
          onCancel={() => setConfirm(null)}
          onConfirm={() => {
            setConfirm(null);
            stop(confirmSession.id);
          }}
        />
      )}
    </>
  );
}
