import { useRef, useState } from 'react';
import type { KeyboardEvent, PointerEvent as ReactPointerEvent, ReactNode } from 'react';
import { useClient } from '../lib/client/context';
import type { AgentKind, Session, SessionId } from '../lib/client/types';
import { startSession } from '../lib/state/actions';
import { emitUiCommand } from '../lib/ui/commands';
import { getSelectedSessionId, selectSession, useSelectedSessionId } from '../lib/ui/selection';
import { moveTab, saveTabOrder, useProjectTabs } from '../lib/ui/tabOrder';
import { openSettings } from '../sheets/overlay';
import { ConfirmDialog } from '../sidebar/ConfirmDialog';
import { ContextMenu } from '../sidebar/ContextMenu';
import type { MenuEntry } from '../sidebar/ContextMenu';
import { InlineEdit } from '../sidebar/InlineEdit';
import { sessionMenu } from '../sidebar/menus';
import { ProjectTile } from './ProjectTile';
import { dragTabOverPanes } from './splitBus';
import { StatusGlyph } from './StatusGlyph';
import { useSessions } from './useSessions';

const noop = () => {};

/** A tab follows the pointer only after it moved this far, so a plain click never reorders. */
const DRAG_PX = 4;

type Menu = { kind: 'tab'; id: SessionId; x: number; y: number } | { kind: 'plus'; x: number; y: number };

/** Where a dragged tab would land: before this tab (`null`: at the end), with the indicator at `left` px of the strip. */
interface Drop {
  id: SessionId;
  before: SessionId | null;
  left: number;
}

// Monochrome 16 px glyphs in the mock's icon style (1.4 stroke, currentColor). No brand logos.
const Glyph = ({ d, circles }: { d: string; circles?: [number, number][] }) => (
  <svg width="14" height="14" viewBox="0 0 16 16" fill="none" stroke="currentColor" strokeWidth="1.4" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
    <path d={d} />
    {circles?.map(([cx, cy]) => <circle key={`${cx}${cy}`} cx={cx} cy={cy} r="1.5" />)}
  </svg>
);
const ICON: Record<AgentKind | 'session' | 'settings', ReactNode> = {
  terminal: <Glyph d="M2 3.5h12v9H2zM4.5 6.5 6.5 8l-2 1.5M8 10h3" />,
  claude_code: <Glyph d="M8 2l1.5 4.5L14 8l-4.5 1.5L8 14l-1.5-4.5L2 8l4.5-1.5z" />,
  codex: <Glyph d="M6 5 3 8l3 3M10 5l3 3-3 3" />,
  opencode: <Glyph d="M5.5 3H4v10h1.5M10.5 3H12v10h-1.5" />,
  session: <Glyph d="M8 3.5v9M3.5 8h9" />,
  settings: <Glyph d="M2.5 5h5M12.5 5h1M2.5 11h1M8.5 11h5" circles={[[10, 5], [6, 11]]} />,
};

/** Tabs of the selected session's project (one tab per session, like the mock) and the "+" button with its menu. */
export function TabStrip() {
  const client = useClient();
  const { sessions, projects } = useSessions();
  const selected = useSelectedSessionId();
  const current = sessions.find((s) => s.id === selected);
  const project = projects.find((p) => p.id === current?.project_id);
  const tabs = useProjectTabs(sessions, current?.project_id);

  const listRef = useRef<HTMLDivElement>(null);
  const suppressClick = useRef(false);
  const [menu, setMenu] = useState<Menu | null>(null);
  const [renaming, setRenaming] = useState<SessionId | null>(null);
  const [confirm, setConfirm] = useState<SessionId | null>(null);
  const [drop, setDrop] = useState<Drop | null>(null);

  const tabEl = (id: SessionId) => listRef.current?.querySelector<HTMLElement>(`[data-tab="${id}"]`);

  const openTabMenu = (id: SessionId, x: number, y: number) => setMenu({ kind: 'tab', id, x, y });

  const onKeyDown = (e: KeyboardEvent<HTMLDivElement>) => {
    const tab = (e.target as HTMLElement).closest<HTMLElement>('[data-tab]');
    if ((e.key === 'ContextMenu' || (e.shiftKey && e.key === 'F10')) && tab?.dataset.tab) {
      e.preventDefault();
      const r = tab.getBoundingClientRect();
      openTabMenu(tab.dataset.tab, r.left + 8, r.bottom - 4);
      return;
    }
    if (e.key !== 'ArrowLeft' && e.key !== 'ArrowRight') return;
    const at = tabs.findIndex((s) => s.id === selected);
    const next = tabs[(at + (e.key === 'ArrowRight' ? 1 : -1) + tabs.length) % tabs.length];
    if (!next) return;
    e.preventDefault();
    selectSession(next.id);
    tabEl(next.id)?.focus();
  };

  const endRename = (id: SessionId) => {
    setRenaming(null);
    // Keyboard users keep their place; do not steal focus from wherever the user clicked next.
    setTimeout(() => {
      if (!document.activeElement || document.activeElement === document.body) tabEl(id)?.focus();
    }, 0);
  };
  const rename = (id: SessionId, title: string) => {
    endRename(id);
    client.renameSession(id, title).catch(noop);
  };
  const stop = (s: Session) =>
    s.status === 'run' || s.status === 'perm' ? setConfirm(s.id) : void client.closeSession(s.id).catch(noop);

  /** Which tab the pointer is left of (`null`: past the last one), and where to draw the insertion bar. */
  const dropAt = (x: number, moving: SessionId): Pick<Drop, 'before' | 'left'> => {
    const box = listRef.current?.getBoundingClientRect();
    const others = [...(listRef.current?.querySelectorAll<HTMLElement>('[data-tab]') ?? [])].filter((el) => el.dataset.tab !== moving);
    for (const el of others) {
      const r = el.getBoundingClientRect();
      if (x < r.left + r.width / 2) return { before: el.dataset.tab ?? null, left: r.left - (box?.left ?? 0) - 2 };
    }
    const last = others.at(-1)?.getBoundingClientRect();
    return { before: null, left: last ? last.right - (box?.left ?? 0) + 2 : 0 };
  };

  // Mouse down selects the tab at once (like a browser's); moving on from there drags it to a new place.
  const onTabPointerDown = (e: ReactPointerEvent<HTMLElement>, s: Session) => {
    if (e.button !== 0 || renaming === s.id) return;
    const before = getSelectedSessionId();
    selectSession(s.id);
    const x0 = e.clientX;
    const y0 = e.clientY;
    const projectId = s.project_id;
    const order = tabs.map((t) => t.id);
    let moved = false;
    let target: SessionId | null = null;
    // Pulled down onto the panes, the tab splits a pane instead of reordering (the panes say where the pointer is).
    let over = false;
    let at = { x: x0, y: y0 };
    const move = (ev: PointerEvent) => {
      if (!moved && Math.hypot(ev.clientX - x0, ev.clientY - y0) < DRAG_PX) return;
      if (!moved) document.body.style.userSelect = 'none';
      moved = true;
      at = { x: ev.clientX, y: ev.clientY };
      over = dragTabOverPanes({ phase: 'move', sessionId: s.id, ...at });
      if (over) return setDrop(null);
      const place = dropAt(ev.clientX, s.id);
      target = place.before;
      setDrop({ id: s.id, ...place });
    };
    const end = (commit: boolean) => {
      window.removeEventListener('pointermove', move);
      window.removeEventListener('pointerup', up);
      window.removeEventListener('pointercancel', cancel);
      window.removeEventListener('keydown', key);
      document.body.style.userSelect = '';
      setDrop(null);
      if (!moved) return;
      // The click that follows a drag must not re-select or hand focus over.
      suppressClick.current = true;
      setTimeout(() => (suppressClick.current = false), 0);
      if (over) return void dragTabOverPanes(commit ? { phase: 'drop', sessionId: s.id, ...at, restore: before } : { phase: 'cancel' });
      const next = moveTab(order, s.id, target);
      if (commit && next.some((id, i) => id !== order[i])) saveTabOrder(projectId, next);
    };
    const up = () => end(true);
    const cancel = () => end(false);
    const key = (ev: globalThis.KeyboardEvent) => ev.key === 'Escape' && end(false);
    window.addEventListener('pointermove', move);
    window.addEventListener('pointerup', up);
    window.addEventListener('pointercancel', cancel);
    window.addEventListener('keydown', key);
  };

  const start = (agent: AgentKind) => void startSession(client, { projects, sessions }, agent).catch(noop);
  const plusMenu = (): MenuEntry[] => [
    { kind: 'item', icon: ICON.terminal, label: 'New Terminal', hint: '⌘T', onSelect: () => start('terminal') },
    { kind: 'separator' },
    { kind: 'item', icon: ICON.claude_code, label: 'Claude Code', hint: '⌥⌘T', onSelect: () => start('claude_code') },
    { kind: 'item', icon: ICON.codex, label: 'Codex', onSelect: () => start('codex') },
    { kind: 'item', icon: ICON.opencode, label: 'OpenCode', onSelect: () => start('opencode') },
    { kind: 'separator' },
    { kind: 'item', icon: ICON.session, label: 'New session…', onSelect: () => emitUiCommand('openNewSession', current?.project_id) },
    { kind: 'item', icon: ICON.settings, label: 'Agent settings…', onSelect: () => openSettings('Agents') },
  ];

  const menuTab = menu?.kind === 'tab' ? sessions.find((s) => s.id === menu.id) : undefined;
  const confirmTab = confirm ? sessions.find((s) => s.id === confirm) : undefined;

  return (
    <>
      <div
        ref={listRef}
        className="relative flex min-w-0 items-end gap-1"
        role="tablist"
        aria-label={project ? `Sessions in ${project.name}` : 'Sessions'}
        onKeyDown={onKeyDown}
      >
        {tabs.map((s) => {
          const active = s.id === selected;
          const cls = `flex h-8 min-w-0 items-center gap-2 rounded-t-md border border-b-0 px-3 text-[12px] ${
            active ? 'border-line bg-term' : 'border-transparent opacity-60 hover:opacity-100'
          } ${drop?.id === s.id ? '!opacity-40' : ''}`;
          const body = (
            <>
              <StatusGlyph status={s.status} />
              {renaming === s.id ? (
                <InlineEdit className="w-[160px] flex-none !text-[12px]" value={s.title} label="Session title" onCommit={(t) => rename(s.id, t)} onCancel={() => endRename(s.id)} />
              ) : (
                <span className="max-w-[200px] truncate">{s.title}</span>
              )}
              {project && (
                <span className="inline-flex shrink-0 items-center gap-1 text-tdim">
                  <ProjectTile project={project} />
                  {project.name}
                </span>
              )}
            </>
          );
          const common = {
            role: 'tab',
            'aria-selected': active,
            tabIndex: active ? 0 : -1,
            'data-tab': s.id,
            className: cls,
            onPointerDown: (e: ReactPointerEvent<HTMLElement>) => onTabPointerDown(e, s),
            onContextMenu: (e: { preventDefault: () => void; stopPropagation: () => void; clientX: number; clientY: number }) => {
              e.preventDefault();
              e.stopPropagation();
              openTabMenu(s.id, e.clientX, e.clientY);
            },
          };
          // An input cannot live inside a button: the tab being renamed is a div.
          if (renaming === s.id) return <div key={s.id} {...common}>{body}</div>;
          return (
            <button
              key={s.id}
              {...common}
              onClick={(e) => {
                if (suppressClick.current) return;
                // A click hands the keyboard to the terminal; arrow keys keep it on the tab (see TerminalView).
                // A new session's terminal takes focus itself once the tab is blurred; the open one is told to.
                selectSession(s.id);
                e.currentTarget.blur();
                if (active) document.querySelector<HTMLElement>('[data-pane][data-focused="true"] textarea')?.focus();
              }}
            >
              {body}
            </button>
          );
        })}
        {drop && (
          <span
            aria-hidden="true"
            data-testid="tab-drop-indicator"
            className="pointer-events-none absolute top-1 bottom-0 w-0.5 rounded-full bg-blue"
            style={{ left: drop.left }}
          />
        )}
      </div>
      <button
        className="mb-2 grid h-[26px] w-7 shrink-0 place-items-center rounded-md text-dim hover:bg-sel"
        onClick={(e) => {
          const r = e.currentTarget.getBoundingClientRect();
          setMenu({ kind: 'plus', x: r.left, y: r.bottom + 4 });
        }}
        aria-label="New session"
        aria-haspopup="menu"
        aria-expanded={menu?.kind === 'plus'}
        title="New terminal ⌘T · more agents"
      >
        +
      </button>

      {menu?.kind === 'plus' && <ContextMenu x={menu.x} y={menu.y} label="New session" entries={plusMenu()} onClose={() => setMenu(null)} />}
      {menu && menuTab && (
        <ContextMenu
          x={menu.x}
          y={menu.y}
          label={`${menuTab.title} actions`}
          entries={sessionMenu(menuTab, {
            open: () => selectSession(menuTab.id),
            split: () => emitUiCommand('splitSession', menuTab.id),
            splitDown: () => emitUiCommand('splitSessionDown', menuTab.id),
            rename: () => setRenaming(menuTab.id),
            stop: () => stop(menuTab),
          })}
          onClose={() => setMenu(null)}
        />
      )}
      {confirmTab && (
        <ConfirmDialog
          title={`Stop ${confirmTab.title}?`}
          body="The session is still working. Stopping ends its terminal and any running command."
          confirmLabel="Stop"
          onCancel={() => setConfirm(null)}
          onConfirm={() => {
            setConfirm(null);
            client.closeSession(confirmTab.id).catch(noop);
          }}
        />
      )}
    </>
  );
}
