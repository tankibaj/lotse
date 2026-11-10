import { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import { FolderGit2, Plus } from 'lucide-react';
import type { KeyboardEvent as ReactKeyboardEvent } from 'react';
import { useClient } from '../lib/client/context';
import type { PermDecision, ProjectId, SessionId } from '../lib/client/types';
import {
  AGENT_LABEL,
  filterGroups,
  groupSessions,
  groupWorkspaces,
  needYouCount,
  shortBranch,
  timeSignature,
  toggleCollapsed,
  toggleNeedFilter,
  useCoarseNow,
  useUiState,
  useWorkspace,
  visibleSessionIds,
} from '../lib/state';
import type { ProjectGroup } from '../lib/state';
import { emitUiCommand } from '../lib/ui/commands';
import { NewSessionMenu } from '../lib/ui/NewSessionMenu';
import { hostBridge } from '../sheets/hostBridge';
import { selectSession, useSelectedSessionId } from '../lib/ui/selection';
import { childOutcome, nestChildren } from './children';
import { ConfirmDialog } from './ConfirmDialog';
import { ContextMenu } from './ContextMenu';
import { projectMenu, sessionMenu, workspaceMenu } from './menus';
import { groupByCheckout } from './checkouts';
import { ProjectRow } from './ProjectRow';
import { SessionCard } from './SessionCard';
import { WorkspaceNameDialog } from './WorkspaceNameDialog';
import { WorkspaceRow } from './WorkspaceRow';

/** `newSession`: the new-session dropdown for project `id`. */
type Menu = { kind: 'project' | 'session' | 'workspace' | 'newSession'; id: string; x: number; y: number };
type Target = { kind: 'project' | 'session' | 'workspace'; id: string };

const noop = () => {};

/** Search, Projects header (both fixed) and the project tree (the only part that scrolls). */
export function Sidebar() {
  const client = useClient();
  const { workspaces, projects, sessions, settings } = useWorkspace();
  const { needFilter, collapsed } = useUiState();
  const selectedId = useSelectedSessionId();

  const navRef = useRef<HTMLElement>(null);
  const hovering = useRef(false);
  const [menu, setMenu] = useState<Menu | null>(null);
  const [renaming, setRenaming] = useState<Target | null>(null);
  const [confirm, setConfirm] = useState<Target | null>(null);
  /** "New Workspace…": `moveProject` goes into it once created (from a project's "Move to Workspace"). */
  const [newWorkspace, setNewWorkspace] = useState<{ moveProject?: ProjectId } | null>(null);
  const [notice, setNotice] = useState<string | null>(null);
  // Sessions whose Allow / Always / Deny is on its way to the daemon: the buttons stay off meanwhile.
  const [answering, setAnswering] = useState<ReadonlySet<SessionId>>(new Set());

  // Time labels are coarse and only change the sidebar when their text changes, never under the pointer or focus.
  const signature = useCallback((n: number) => timeSignature(sessions, n), [sessions]);
  const hold = useCallback(() => hovering.current || (navRef.current?.contains(document.activeElement) ?? false), []);
  const now = useCoarseNow(signature, hold);

  const needCount = needYouCount(sessions);
  const filterOn = needFilter && needCount > 0;
  // Child sessions sit under their parent ("started by …"); `nested` are the ones indented under a parent card.
  const { groups, nested } = useMemo(() => {
    const nested = new Set<SessionId>();
    const groups = groupSessions(projects, sessions).map((g) => ({
      project: g.project,
      sessions: nestChildren(g.sessions).map((n) => {
        if (n.child) nested.add(n.session.id);
        return n.session;
      }),
    }));
    return { groups, nested };
  }, [projects, sessions]);
  const sessionById = useMemo(() => new Map(sessions.map((s) => [s.id, s])), [sessions]);
  const outcomes = useMemo(() => {
    const m = new Map<SessionId, { done: number; fail: number }>();
    for (const s of sessions) if (s.parent_session && !m.has(s.parent_session)) m.set(s.parent_session, childOutcome(s.parent_session, sessions));
    return m;
  }, [sessions]);
  const shown = useMemo(() => filterGroups(groups, filterOn), [groups, filterOn]);
  const projectById = useMemo(() => new Map(projects.map((p) => [p.id, p])), [projects]);
  // Workspace headers appear only when there is more than one workspace: with one, the tree is the mock's.
  const multi = workspaces.length > 1;
  const wsGroups = useMemo(() => groupWorkspaces(workspaces, shown), [workspaces, shown]);
  const wsOpen = (id: string) => filterOn || !collapsed.has(id);
  /** Project groups in the order the sidebar shows them, collapsed workspaces left out. */
  const navGroups = (multi ? wsGroups.filter((w) => wsOpen(w.workspace.id)).flatMap((w) => w.groups) : shown);

  const onSelect = useCallback((id: SessionId) => selectSession(id), []);
  const onToggle = useCallback((id: ProjectId) => toggleCollapsed(id), []);
  const onSessionMenu = useCallback((id: SessionId, x: number, y: number) => setMenu({ kind: 'session', id, x, y }), []);
  const onProjectMenu = useCallback((id: ProjectId, x: number, y: number) => setMenu({ kind: 'project', id, x, y }), []);
  const onWorkspaceMenu = useCallback((id: string, x: number, y: number) => setMenu({ kind: 'workspace', id, x, y }), []);
  // Keyboard users keep their place: after an inline action the control they used disappears, so focus
  // goes back to the card (or project / workspace row) it belonged to.
  const refocus = useCallback((t: Target) => {
    setTimeout(() => {
      const sel = t.kind === 'session' ? `[data-sid="${t.id}"] [data-card-main]` : t.kind === 'project' ? `[data-pid="${t.id}"]` : `[data-wid="${t.id}"]`;
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
  const onRenameWorkspace = useCallback(
    (id: string, name: string) => {
      endRename();
      client.renameWorkspace(id, name).catch((e: unknown) => setNotice(e instanceof Error ? e.message : 'Could not rename the workspace'));
    },
    [client, endRename],
  );
  const cancelRename = endRename;

  const stop = (id: SessionId) => client.closeSession(id).catch(noop);
  const fail = (what: string) => (e: unknown) => setNotice(e instanceof Error ? e.message : what);

  const onKeyDown = (e: ReactKeyboardEvent<HTMLElement>) => {
    const el = e.target as HTMLElement;
    if (el instanceof HTMLInputElement) return;
    const card = el.closest<HTMLElement>('[data-sid]');
    const row = el.closest<HTMLElement>('[data-pid]');
    const wsRow = el.closest<HTMLElement>('[data-wid]');
    if (e.key === 'ArrowDown' || e.key === 'ArrowUp') {
      const ids = visibleSessionIds(navGroups, collapsed, filterOn);
      const down = e.key === 'ArrowDown';
      let next: SessionId | undefined;
      if (!card && wsRow) {
        // Workspace header focused: the session right after / before it, like a project row.
        const at = wsGroups.findIndex((w) => w.workspace.id === wsRow.dataset.wid);
        const above = wsGroups.slice(0, Math.max(0, at)).filter((w) => wsOpen(w.workspace.id)).flatMap((w) => w.groups);
        const before = visibleSessionIds(above, collapsed, filterOn).length;
        next = ids[down ? before : before - 1];
      } else if (!card && row) {
        // Project row focused: step to the session right after / before the row, not relative to the selection.
        const at = navGroups.findIndex((g) => g.project.id === row.dataset.pid);
        const before = visibleSessionIds(navGroups.slice(0, Math.max(0, at)), collapsed, filterOn).length;
        next = ids[down ? before : before - 1];
      } else {
        const i = ids.indexOf(card?.dataset.sid ?? selectedId ?? '');
        next = ids[Math.min(ids.length - 1, Math.max(0, i + (down ? 1 : -1)))];
      }
      if (!next) return;
      e.preventDefault();
      selectSession(next);
      navRef.current?.querySelector<HTMLElement>(`[data-sid="${next}"] [data-card-main]`)?.focus();
    } else if ((row ?? wsRow) && !filterOn && (e.key === 'ArrowRight' || e.key === 'ArrowLeft')) {
      e.preventDefault();
      toggleCollapsed((row ?? wsRow)?.dataset.pid ?? wsRow?.dataset.wid ?? '', e.key === 'ArrowLeft');
    } else if (e.key === 'ContextMenu' || (e.shiftKey && e.key === 'F10')) {
      const target = card ?? row ?? wsRow;
      if (!target) return;
      e.preventDefault();
      const r = target.getBoundingClientRect();
      if (card?.dataset.sid) onSessionMenu(card.dataset.sid, r.left + 24, r.bottom - 4);
      else if (row?.dataset.pid) onProjectMenu(row.dataset.pid, r.left + 24, r.bottom - 4);
      else if (wsRow?.dataset.wid) onWorkspaceMenu(wsRow.dataset.wid, r.left + 24, r.bottom - 4);
    }
  };

  const menuProject = menu?.kind === 'project' ? projectById.get(menu.id) : undefined;
  const menuSession = menu?.kind === 'session' ? sessions.find((s) => s.id === menu.id) : undefined;
  const menuWorkspace = menu?.kind === 'workspace' ? workspaces.find((w) => w.id === menu.id) : undefined;
  const confirmProject = confirm?.kind === 'project' ? projectById.get(confirm.id) : undefined;
  const confirmSession = confirm?.kind === 'session' ? sessions.find((s) => s.id === confirm.id) : undefined;
  const confirmWorkspace = confirm?.kind === 'workspace' ? workspaces.find((w) => w.id === confirm.id) : undefined;
  const confirmWorkspaceProjects = confirmWorkspace ? projects.filter((p) => p.workspace_id === confirmWorkspace.id).length : 0;

  const startedBy = (parent?: SessionId) => {
    const p = parent ? sessionById.get(parent) : undefined;
    return p ? p.title || AGENT_LABEL[p.agent] : undefined;
  };

  /** One project: its row, the new-session row of an empty project, and its cards. */
  const renderProject = ({ project, sessions: list }: ProjectGroup) => {
    const open = filterOn || !collapsed.has(project.id);
    // One checkout (the common case): its branch and changes sit on the project row. More: each worktree gets a header.
    const checkouts = groupByCheckout(list, (x) => nested.has(x.id));
    const mainCheckout = checkouts.find((g) => g.main);
    return (
      <div key={project.id} className="mt-3 space-y-0.5 first:mt-0" data-project={project.id}>
        <ProjectRow
          project={project}
          sessions={list}
          open={open}
          main={mainCheckout?.summary ? { branch: mainCheckout.branch, ...mainCheckout.summary } : null}
          renaming={renaming?.kind === 'project' && renaming.id === project.id}
          onToggle={onToggle}
          onMenu={onProjectMenu}
          onRename={onRenameProject}
          onCancelRename={cancelRename}
        />
        {open && list.length === 0 && (
          // an empty project needs no message: one quiet action, lined up with where its session cards would be
          <div className="ml-3">
            <button
              className="ml-3 mr-1 flex w-[calc(100%-1rem)] items-center gap-1.5 rounded-md px-2 py-1 text-left text-[12px] text-dim hover:bg-black/5 hover:text-blue dark:hover:bg-white/5"
              aria-haspopup="menu"
              onClick={(e) => {
                const r = e.currentTarget.getBoundingClientRect();
                setMenu({ kind: 'newSession', id: project.id, x: r.left, y: r.bottom + 4 });
              }}
            >
              <Plus size={12} strokeWidth={2} aria-hidden="true" />
              New session
            </button>
          </div>
        )}
        {open &&
          checkouts.map((g) => (
            <div key={g.key} className="ml-3 space-y-0.5">
              {!g.main && (
                // a worktree: its own branch and changes, so its sessions are told apart from the main checkout's
                <div
                  data-checkout={g.key}
                  className="mt-1.5 flex min-w-0 items-center gap-1.5 px-2 text-[11px] text-dim"
                  title={`Worktree: ${g.sessions[0]?.git.worktree_path ?? g.branch}`}
                  aria-label={`Worktree ${g.branch}`}
                >
                  <FolderGit2 size={12} strokeWidth={2} className="flex-none" aria-hidden="true" />
                  <span className="min-w-0 truncate font-mono" title={g.branch}>{shortBranch(g.branch)}</span>
                  {g.summary && g.summary.dirty > 0 && <span className="shrink-0 tabular-nums">±{g.summary.dirty}</span>}
                  {g.summary && (g.summary.added > 0 || g.summary.removed > 0) && (
                    <span className="shrink-0 tabular-nums">
                      <span className="text-green">+{g.summary.added}</span> <span className="text-red">−{g.summary.removed}</span>
                    </span>
                  )}
                </div>
              )}
              {g.sessions.map((s, i) => (
                <SessionCard
                  key={s.id}
                  session={s}
                  project={project}
                  selected={s.id === selectedId}
                  now={now}
                  child={nested.has(s.id)}
                  lastChild={nested.has(s.id) && !(g.sessions[i + 1] && nested.has(g.sessions[i + 1]?.id ?? ''))}
                  startedBy={startedBy(s.parent_session)}
                  childrenDone={outcomes.get(s.id)?.done}
                  childrenFailed={outcomes.get(s.id)?.fail}
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
          ))}
      </div>
    );
  };

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
        tabIndex={-1} // a scroller must not become a Tab stop of its own (WebKit/Chromium make some focusable); the rows are the stops
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
        {multi
          ? wsGroups
              .filter((w) => !filterOn || w.groups.length > 0)
              .map((w) => {
                const open = wsOpen(w.workspace.id);
                return (
                  <div key={w.workspace.id} className="space-y-0.5" data-workspace={w.workspace.id}>
                    <WorkspaceRow
                      workspace={w.workspace}
                      sessions={w.sessions}
                      projectCount={w.groups.length}
                      open={open}
                      renaming={renaming?.kind === 'workspace' && renaming.id === w.workspace.id}
                      onToggle={onToggle}
                      onMenu={onWorkspaceMenu}
                      onRename={onRenameWorkspace}
                      onCancelRename={cancelRename}
                    />
                    {open && w.groups.length === 0 && <div className="py-1 pl-[26px] text-[12px] text-dim">No projects</div>}
                    {open && w.groups.map(renderProject)}
                  </div>
                );
              })
          : shown.map(renderProject)}
      </nav>

      {menu && menuProject && (
        <ContextMenu
          x={menu.x}
          y={menu.y}
          label={`${menuProject.name} actions`}
          entries={projectMenu(menuProject, settings, client, {
            // the project menu closes first, then the dropdown opens where it was
            newSession: () => setMenu({ kind: 'newSession', id: menuProject.id, x: menu.x, y: menu.y }),
            changeStyle: () => emitUiCommand('openProjectStyle', menuProject.id),
            workspaces,
            moveTo: (workspaceId) => void client.moveProject(menuProject.id, workspaceId).catch(fail('Could not move the project')),
            newWorkspace: () => setNewWorkspace({ moveProject: menuProject.id }),
            reveal: hostBridge().revealInFinder
              ? () => void hostBridge().revealInFinder?.(menuProject.path)?.catch(noop)
              : undefined,
            rename: () => setRenaming({ kind: 'project', id: menuProject.id }),
            remove: () => setConfirm({ kind: 'project', id: menuProject.id }),
          })}
          onClose={() => setMenu(null)}
        />
      )}
      {menu && menuWorkspace && (
        <ContextMenu
          x={menu.x}
          y={menu.y}
          label={`${menuWorkspace.name} actions`}
          entries={workspaceMenu(menuWorkspace, {
            newWorkspace: () => setNewWorkspace({}),
            rename: () => setRenaming({ kind: 'workspace', id: menuWorkspace.id }),
            // the first workspace is the default one: it stays
            remove: menuWorkspace.id === workspaces[0]?.id ? undefined : () => setConfirm({ kind: 'workspace', id: menuWorkspace.id }),
          })}
          onClose={() => setMenu(null)}
        />
      )}
      {newWorkspace && (
        <WorkspaceNameDialog
          title="New Workspace"
          submitLabel="Create"
          onCancel={() => setNewWorkspace(null)}
          onSubmit={async (name) => {
            const ws = await client.createWorkspace(name);
            setNewWorkspace(null);
            if (newWorkspace.moveProject) await client.moveProject(newWorkspace.moveProject, ws.id).catch(fail('Could not move the project'));
          }}
        />
      )}
      {menu?.kind === 'newSession' && (
        <NewSessionMenu x={menu.x} y={menu.y} projectId={menu.id} heading={projectById.get(menu.id)?.name} onClose={() => setMenu(null)} />
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
      {confirmWorkspace && (
        <ConfirmDialog
          title={`Delete ${confirmWorkspace.name}?`}
          body={
            confirmWorkspaceProjects > 0
              ? `Its ${confirmWorkspaceProjects} project${confirmWorkspaceProjects === 1 ? '' : 's'} move to ${workspaces[0]?.name ?? 'the default workspace'}. Nothing is removed from disk.`
              : 'The workspace is empty.'
          }
          confirmLabel="Delete"
          onCancel={() => setConfirm(null)}
          onConfirm={() => {
            setConfirm(null);
            client.deleteWorkspace(confirmWorkspace.id, true).catch(fail('Could not delete the workspace'));
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
