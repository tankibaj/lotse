import { Fragment, useCallback, useEffect, useRef, useState } from 'react';
import type { DragEvent, KeyboardEvent as ReactKeyboardEvent, PointerEvent as ReactPointerEvent } from 'react';
import { X } from 'lucide-react';
import type { Project, Session, SessionId } from '../lib/client';
import { onUiCommand } from '../lib/ui/commands';
import { selectSession, useSelectedSessionId } from '../lib/ui/selection';
import { TerminalView } from '../terminal';
import { useTerminalAppearance } from '../terminal/useTerminalAppearance';
import { EditorPane } from './EditorPane';
import { ProjectTile } from './ProjectTile';
import { StatusGlyph } from './StatusGlyph';
import {
  MAX_PANES,
  addEditor,
  addPane,
  closePane,
  columnsOf,
  dropDirection,
  emptyLayout,
  focusPane,
  focusStep,
  heightOf,
  isEditor,
  leftShare,
  nextSessionToOpen,
  paneKey,
  resizePair,
  resizeStack,
  sessionOf,
  sessionPanes,
  syncLayout,
  topShare,
  weightOf,
} from './layout';
import type { PaneLayout, PaneRef, SplitDirection } from './layout';
import { onSplitRequest, onTabDrag } from './splitBus';
import { useSessions } from './useSessions';

/** Drag type the sidebar card sets (spec/mock.html: `text/lotse-session`). */
export const SESSION_DRAG_TYPE = 'text/lotse-session';

const AGENT_LABEL: Record<Session['agent'], string> = {
  terminal: 'Terminal',
  claude_code: 'Claude Code',
  codex: 'Codex',
  opencode: 'OpenCode',
};
/** Arrow keys on a focused divider move it by this many px. */
const KEY_RESIZE_PX = 24;
/** The drop zone sits this far inside the half of the pane it covers. */
const ZONE_INSET_PX = 12;

/** A keystroke inside an editor pane (its text area is a text field, so the shortcuts below skip it). */
const inEditorPane = (target: EventTarget | null): target is HTMLElement => target instanceof HTMLElement && target.closest('[data-editor-pane]') !== null;

/** Is the key event aimed at a text field that is not a terminal? Then app shortcuts stay out of its way. */
function inForeignTextField(target: EventTarget | null): boolean {
  if (!(target instanceof HTMLElement)) return false;
  if (target.closest('.xterm')) return false;
  return target.isContentEditable || ['INPUT', 'TEXTAREA', 'SELECT'].includes(target.tagName);
}

interface Notice {
  text: string;
  id: number;
}

const sizeOf = (el: HTMLElement, vertical: boolean) => (vertical ? el.getBoundingClientRect().height : el.getBoundingClientRect().width);

/**
 * Pointer drag on the divider between `a` and `b` (`vertical`: a horizontal divider, so heights count; else widths).
 * `onSize` gets the wanted size of `a` and the measured size of the pair on every move.
 */
function trackDrag(e: ReactPointerEvent<HTMLDivElement>, a: HTMLElement, b: HTMLElement, vertical: boolean, onSize: (firstPx: number, totalPx: number) => void) {
  e.preventDefault();
  const start = vertical ? e.clientY : e.clientX;
  const first = sizeOf(a, vertical);
  const total = first + sizeOf(b, vertical);
  document.body.style.userSelect = 'none';
  document.body.style.cursor = vertical ? 'row-resize' : 'col-resize';
  const move = (ev: PointerEvent) => onSize(first + (vertical ? ev.clientY : ev.clientX) - start, total);
  const up = () => {
    document.body.style.userSelect = '';
    document.body.style.cursor = '';
    window.removeEventListener('pointermove', move);
    window.removeEventListener('pointerup', up);
  };
  window.addEventListener('pointermove', move);
  window.addEventListener('pointerup', up);
}

/**
 * Where a dragged session would open: `direction` of the pane at flat `index` (null: the focused pane), drawn as
 * `rect` (px inside the panes area; null: no pane geometry, so the right half of the area).
 */
interface Zone {
  direction: SplitDirection;
  index: number | null;
  rect: { left: number; top: number; width: number; height: number } | null;
}

const sameZone = (a: Zone | null, b: Zone | null) =>
  a === b ||
  (a !== null &&
    b !== null &&
    a.direction === b.direction &&
    a.index === b.index &&
    a.rect?.left === b.rect?.left &&
    a.rect?.top === b.rect?.top &&
    a.rect?.width === b.rect?.width &&
    a.rect?.height === b.rect?.height);

const ZONE_LABEL: Record<SplitDirection, string> = { right: 'Open side by side', down: 'Open below' };

/**
 * The terminal area: up to four sessions in columns side by side, a column holding one or more panes stacked,
 * with draggable dividers. The selected session (`lib/ui/selection`) is the focused pane; focusing a pane selects
 * its session.
 * Keys: ⌘D split right, ⌘⇧D split down, ⌘W close pane, ⌘[ ⌘] focus (column by column, top to bottom), ⌘+ ⌘− ⌘0 font size.
 * ⌥-click (⌥⇧-click: down) on a session card, the card's "Open in Split Right / Down", dropping a card or a tab on
 * the bottom or right half of a pane all end in `openInSplit` / `SESSION_DRAG_TYPE` / `onTabDrag`.
 * Files open as editor panes beside the session's terminal (UI command `openFile`); ⌘W ⌘[ ⌘] work inside them,
 * ⌥⌘↩ expands the editor to the whole area and Esc restores.
 */
export function Panes() {
  const { sessions, projects, loaded } = useSessions();
  const selected = useSelectedSessionId();
  const appearance = useTerminalAppearance();
  const [layout, setLayout] = useState<PaneLayout>(emptyLayout);
  const [zone, setZone] = useState<Zone | null>(null);
  const [notice, setNotice] = useState<Notice | null>(null);
  const [focusSignal, setFocusSignal] = useState(0);
  const [expanded, setExpanded] = useState<string | null>(null);
  const [dirty, setDirty] = useState<ReadonlySet<string>>(new Set());
  const discard = useRef<string | null>(null);
  const box = useRef<HTMLDivElement>(null);

  // Keep panes in step with the sessions that exist and the selection (derived while rendering).
  if (loaded) {
    const synced = syncLayout(layout, selected, (id) => sessions.some((s) => s.id === id));
    if (synced !== layout) setLayout(synced);
  }

  // Nothing selected yet (or the selected session is gone): pick the first one. The sidebar normally does this first.
  const selectedExists = selected !== null && sessions.some((s) => s.id === selected);
  const firstId = sessions[0]?.id ?? null;
  useEffect(() => {
    if (loaded && !selectedExists) selectSession(firstId);
  }, [loaded, selectedExists, firstId]);

  useEffect(() => {
    if (!notice) return;
    const t = window.setTimeout(() => setNotice(null), 1800);
    return () => window.clearTimeout(t);
  }, [notice]);

  // The expanded editor, while it is still open.
  const expandedKey = expanded !== null && layout.panes.some((p) => paneKey(p) === expanded) ? expanded : null;

  const onDirty = useCallback(
    (key: string, is: boolean) =>
      setDirty((prev) => {
        if (prev.has(key) === is) return prev;
        const next = new Set(prev);
        if (is) next.add(key);
        else next.delete(key);
        return next;
      }),
    [],
  );

  const say = useCallback((text: string) => setNotice((n) => ({ text, id: (n?.id ?? 0) + 1 })), []);

  const focusAt = useCallback(
    (index: number) => {
      const next = focusPane(layout, index);
      setLayout(next);
      const ref = next.panes[next.focus];
      if (ref !== undefined) selectSession(sessionOf(ref));
    },
    [layout],
  );

  /**
   * Open `id` beside (`right`) or below (`down`) the pane at flat `index` (null: the focused pane). `restore` is the
   * session that was selected before a tab press selected `id`: a tab dragged onto the panes is not open yet, but that
   * press already swapped it into the focused pane, so put the old session back first.
   */
  const place = useCallback(
    (id: SessionId, direction: SplitDirection, index: number | null, restore: SessionId | null = null) => {
      const exists = (s: SessionId) => sessions.some((x) => x.id === s);
      if (!exists(id)) return;
      let base = layout;
      if (restore !== null && restore !== id && exists(restore) && !base.panes.includes(restore) && base.panes[base.focus] === id) {
        base = syncLayout(base, restore, exists);
      }
      if (index !== null) base = focusPane(base, index);
      const { layout: next, outcome } = addPane(base, id, direction);
      if (outcome === 'full') return say(`Up to ${MAX_PANES} panes side by side`);
      setLayout(next);
      setExpanded(null);
      selectSession(id);
    },
    [layout, sessions, say],
  );

  /** Open `path` of session `sessionId` in an editor pane (its terminal first, when it is not open). */
  const openFile = useCallback(
    (sessionId: SessionId, path: string) => {
      const exists = (s: SessionId) => sessions.some((x) => x.id === s);
      if (!exists(sessionId)) return;
      const { layout: next, outcome } = addEditor(syncLayout(layout, sessionId, exists), sessionId, path);
      if (outcome === 'full') return say(`Up to ${MAX_PANES} panes side by side`);
      setLayout(next);
      setExpanded(null);
      selectSession(sessionId);
    },
    [layout, sessions, say],
  );

  const split = useCallback(
    (wanted: SessionId | null, direction: SplitDirection = 'right') => {
      const id = wanted ?? nextSessionToOpen(sessions, sessionPanes(layout));
      if (id === null) return say('All sessions are already open');
      place(id, direction, null);
    },
    [layout, sessions, say, place],
  );

  const close = useCallback(
    (index: number) => {
      const next = closePane(layout, index);
      if (next === layout) return layout.panes.length === 1 ? say('The last pane stays open') : undefined;
      const key = paneKey(layout.panes[index] as PaneRef);
      // Unsaved edits: the first close only asks, the second throws them away.
      if (dirty.has(key) && discard.current !== key) {
        discard.current = key;
        window.setTimeout(() => (discard.current = discard.current === key ? null : discard.current), 4000);
        return say('Unsaved changes: close again to discard them');
      }
      discard.current = null;
      setLayout(next);
      setFocusSignal((n) => n + 1); // the ✕ that had focus is gone: put the keyboard back in the terminal
      const ref = next.panes[next.focus];
      if (ref !== undefined) selectSession(sessionOf(ref));
    },
    [layout, dirty, say],
  );

  const step = useCallback(
    (delta: number) => {
      const next = focusStep(layout, delta);
      if (next === layout) return;
      setLayout(next);
      setExpanded(null);
      const ref = next.panes[next.focus];
      if (ref !== undefined) selectSession(sessionOf(ref));
    },
    [layout],
  );

  /** The drop zone for a pointer at `x`,`y` over pane element `el` (null: not over a pane). */
  const zoneAt = useCallback((el: HTMLElement | null, x: number, y: number): Zone => {
    const area = box.current;
    const index = el && area ? [...area.querySelectorAll<HTMLElement>('[data-pane]')].indexOf(el) : -1;
    const r = el?.getBoundingClientRect();
    if (!area || !r || index < 0 || r.width <= 0 || r.height <= 0) return { direction: 'right', index: null, rect: null };
    const direction = dropDirection(x - r.left, y - r.top, r.width, r.height);
    const origin = area.getBoundingClientRect();
    const left = r.left - origin.left;
    const top = r.top - origin.top;
    const half = direction === 'down' ? { left, top: top + r.height / 2, width: r.width, height: r.height / 2 } : { left: left + r.width / 2, top, width: r.width / 2, height: r.height };
    return {
      direction,
      index,
      rect: {
        left: half.left + ZONE_INSET_PX,
        top: half.top + ZONE_INSET_PX,
        width: half.width - 2 * ZONE_INSET_PX,
        height: half.height - 2 * ZONE_INSET_PX,
      },
    };
  }, []);

  const showZone = useCallback((next: Zone | null) => setZone((prev) => (sameZone(prev, next) ? prev : next)), []);

  // Other WPs ask for a split through the bus (card context menu, palette).
  useEffect(() => onSplitRequest(split), [split]);
  useEffect(() => onUiCommand('splitSession', (id) => split(id ?? null, 'right')), [split]);
  useEffect(() => onUiCommand('splitSessionDown', (id) => split(id ?? null, 'down')), [split]);
  useEffect(() => onUiCommand('openFile', (id, path) => (id && path ? openFile(id, path) : undefined)), [openFile]);

  // A tab pulled down out of the tab strip: the pane under the pointer decides the zone (the strip drags with the pointer).
  useEffect(
    () =>
      onTabDrag((drag) => {
        if (drag.phase === 'cancel') {
          setZone(null);
          return false;
        }
        const panes = [...(box.current?.querySelectorAll<HTMLElement>('[data-pane]') ?? [])];
        const under = panes.find((p) => {
          const r = p.getBoundingClientRect();
          return r.width > 0 && r.height > 0 && drag.x >= r.left && drag.x < r.right && drag.y >= r.top && drag.y < r.bottom;
        });
        if (!under) {
          setZone(null);
          return false;
        }
        const target = zoneAt(under, drag.x, drag.y);
        if (drag.phase === 'move') {
          showZone(target);
          return true;
        }
        setZone(null);
        place(drag.sessionId, target.direction, target.index, drag.restore);
        return true;
      }),
    [zoneAt, showZone, place],
  );

  // ⌥-click on a session card opens it side by side, ⌥⇧-click below (cards carry `data-session-id`).
  useEffect(() => {
    const onClick = (e: MouseEvent) => {
      if (!e.altKey || !(e.target instanceof Element)) return;
      const id = e.target.closest('[data-session-id]')?.getAttribute('data-session-id');
      if (!id) return;
      e.preventDefault();
      e.stopPropagation();
      split(id, e.shiftKey ? 'down' : 'right');
    };
    document.addEventListener('click', onClick, true);
    return () => document.removeEventListener('click', onClick, true);
  }, [split]);

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (!e.metaKey || e.ctrlKey || inForeignTextField(e.target)) return;
      const handled = (fn: () => void) => {
        e.preventDefault();
        fn();
      };
      // Match the typed character (`key`), not the physical key (`code`): on a German keyboard "+" and "-" sit
      // elsewhere and "[" / "]" need ⌥, so `alt` is allowed. Of the letters only ⌘D takes ⇧ (split down, as in iTerm2).
      const k = e.key.toLowerCase();
      if (!e.shiftKey && !e.altKey) {
        if (k === 'd') return handled(() => split(null, 'right'));
        if (k === 'w') return handled(() => close(layout.focus));
        if (k === '-') return handled(() => appearance.zoom(-1));
        if (k === '0') return handled(appearance.resetZoom);
      }
      if (k === 'd' && e.shiftKey && !e.altKey) return handled(() => split(null, 'down'));
      if (k === '[') return handled(() => step(-1));
      if (k === ']') return handled(() => step(1));
      // ⌘+ types "+" (⌘⇧= on a US keyboard); a bare "=" works too. ⌘⇧0 on a German layout types "=": not a zoom.
      if ((k === '=' && !e.shiftKey) || k === '+') return handled(() => appearance.zoom(1));
    };
    window.addEventListener('keydown', onKey);
    return () => window.removeEventListener('keydown', onKey);
  }, [split, close, step, layout.focus, appearance]);

  // Keys inside an editor pane. Captured, so CodeMirror's own ⌘[ ⌘] (indent) and ⌘W never see them: the pane shortcuts win.
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (!inEditorPane(e.target) || e.ctrlKey) return;
      const key = paneKey(layout.panes[layout.focus] as PaneRef);
      const handled = (fn: () => void) => {
        e.preventDefault();
        e.stopPropagation();
        fn();
      };
      if (e.key === 'Escape' && !e.metaKey && expandedKey !== null && !e.target.closest('.cm-panel')) return handled(() => setExpanded(null));
      if (!e.metaKey) return;
      const k = e.key.toLowerCase();
      if (k === 'enter' && e.altKey && !e.shiftKey && isEditor(layout.panes[layout.focus] as PaneRef)) return handled(() => setExpanded(expandedKey === key ? null : key));
      if (e.altKey && k !== '[' && k !== ']') return;
      if (k === 'w' && !e.shiftKey) return handled(() => close(layout.focus));
      if (k === '[') return handled(() => step(-1));
      if (k === ']') return handled(() => step(1));
    };
    window.addEventListener('keydown', onKey, true);
    return () => window.removeEventListener('keydown', onKey, true);
  }, [layout, expandedKey, close, step]);

  const columnEls = () => box.current?.querySelectorAll<HTMLElement>('[data-column]');
  const paneEls = () => box.current?.querySelectorAll<HTMLElement>('[data-pane]');
  const beginResize = (
    e: ReactPointerEvent<HTMLDivElement>,
    a: HTMLElement,
    b: HTMLElement,
    vertical: boolean,
    resize: (l: PaneLayout, firstPx: number, totalPx: number) => PaneLayout,
  ) => trackDrag(e, a, b, vertical, (px, total) => setLayout((l) => resize(l, px, total)));

  /** Arrow key on a divider between `a` and `b`: move it by `delta` px. */
  const nudge = (a: HTMLElement | undefined, b: HTMLElement | undefined, vertical: boolean, delta: number, resize: (l: PaneLayout, firstPx: number, totalPx: number) => PaneLayout) => {
    if (!a || !b) return;
    const first = sizeOf(a, vertical);
    const total = first + sizeOf(b, vertical);
    setLayout((l) => resize(l, first + delta, total));
  };

  /** Divider before the column at position `col`, whose top pane is flat pane `index`. */
  const dragDivider = (index: number, col: number) => (e: ReactPointerEvent<HTMLDivElement>) => {
    const els = columnEls();
    const a = els?.[col - 1];
    const b = els?.[col];
    if (a && b) beginResize(e, a, b, false, (l, px, total) => resizePair(l, index, px, total));
  };

  const keyDivider = (index: number, col: number) => (e: ReactKeyboardEvent<HTMLDivElement>) => {
    if (e.key !== 'ArrowLeft' && e.key !== 'ArrowRight') return;
    e.preventDefault();
    const els = columnEls();
    nudge(els?.[col - 1], els?.[col], false, e.key === 'ArrowLeft' ? -KEY_RESIZE_PX : KEY_RESIZE_PX, (l, px, total) => resizePair(l, index, px, total));
  };

  /** Divider above the stacked pane at flat `index`. */
  const dragStack = (index: number) => (e: ReactPointerEvent<HTMLDivElement>) => {
    const els = paneEls();
    const a = els?.[index - 1];
    const b = els?.[index];
    if (a && b) beginResize(e, a, b, true, (l, px, total) => resizeStack(l, index, px, total));
  };

  const keyStack = (index: number) => (e: ReactKeyboardEvent<HTMLDivElement>) => {
    if (e.key !== 'ArrowUp' && e.key !== 'ArrowDown') return;
    e.preventDefault();
    const els = paneEls();
    nudge(els?.[index - 1], els?.[index], true, e.key === 'ArrowUp' ? -KEY_RESIZE_PX : KEY_RESIZE_PX, (l, px, total) => resizeStack(l, index, px, total));
  };

  const isSessionDrag = (e: DragEvent) => [...e.dataTransfer.types].includes(SESSION_DRAG_TYPE);
  const zoneOfDrag = (e: DragEvent) => zoneAt(e.target instanceof Element ? e.target.closest<HTMLElement>('[data-pane]') : null, e.clientX, e.clientY);
  const onDragOver = (e: DragEvent) => {
    if (!isSessionDrag(e)) return;
    e.preventDefault();
    showZone(zoneOfDrag(e));
  };
  const onDragLeave = (e: DragEvent) => {
    if (!e.currentTarget.contains(e.relatedTarget as Node | null)) setZone(null);
  };
  const onDrop = (e: DragEvent) => {
    setZone(null);
    const id = e.dataTransfer.getData(SESSION_DRAG_TYPE);
    if (!id) return;
    e.preventDefault();
    const target = zoneOfDrag(e);
    place(id, target.direction, target.index);
  };

  const sessionById = (id: SessionId) => sessions.find((s) => s.id === id);
  /** Name of a pane for the divider labels. */
  const labelOf = (p: PaneRef) => (isEditor(p) ? (p.path.split('/').pop() ?? p.path) : (sessionById(p)?.title ?? ''));
  const hide = (is: boolean) => (is ? { display: 'none' } : undefined);
  const projectOf = (s: Session): Project | undefined => projects.find((p) => p.id === s.project_id);
  const multi = layout.panes.length > 1;
  const columns = columnsOf(layout);
  /** Flat index of each column's top pane. */
  const starts = columns.map((_, c) => columns.slice(0, c).reduce((n, col) => n + col.length, 0));

  return (
    <div
      ref={box}
      className="relative flex min-w-0 flex-1"
      data-testid="panes"
      onDragOver={onDragOver}
      onDragLeave={onDragLeave}
      onDrop={onDrop}
    >
      {layout.panes.length === 0 && (
        <div className="grid min-w-0 flex-1 place-items-center bg-term text-tdim" data-testid="terminal-placeholder" aria-label="Terminal">
          {loaded ? 'No session open' : ''}
        </div>
      )}
      {columns.map((column, c) => {
        const top = column[0] as PaneRef;
        const topKey = paneKey(top);
        const first = starts[c] ?? 0;
        const shut = expandedKey !== null && !column.some((p) => paneKey(p) === expandedKey);
        return (
          <Fragment key={topKey}>
            {c > 0 && (
              <div
                className="relative z-10 w-px shrink-0 cursor-col-resize bg-line focus-visible:bg-blue"
                style={hide(expandedKey !== null)}
                role="separator"
                aria-orientation="vertical"
                aria-label={`Resize panes ${labelOf(columns[c - 1]?.[0] as PaneRef)} and ${labelOf(top)}`}
                aria-valuemin={0}
                aria-valuemax={100}
                aria-valuenow={Math.round(leftShare(layout, first) * 100)}
                tabIndex={0}
                data-testid="pane-divider"
                onPointerDown={dragDivider(first, c)}
                onKeyDown={keyDivider(first, c)}
              >
                <span className="absolute inset-y-0 -right-1.5 -left-1.5" />
              </div>
            )}
            <div className="flex min-h-0 min-w-0 flex-col" style={{ flex: `${weightOf(layout, topKey)} 1 0`, ...hide(shut) }} data-column={topKey}>
              {column.map((ref, r) => {
                const i = first + r;
                const key = paneKey(ref);
                const s = sessionById(sessionOf(ref));
                if (!s) return null;
                const id = s.id;
                const project = projectOf(s);
                const focused = i === layout.focus;
                const editor = isEditor(ref);
                const closed = expandedKey !== null && key !== expandedKey;
                return (
                  <Fragment key={key}>
                    {r > 0 && (
                      <div
                        className="relative z-10 h-px shrink-0 cursor-row-resize bg-line focus-visible:bg-blue"
                        style={hide(expandedKey !== null)}
                        role="separator"
                        aria-orientation="horizontal"
                        aria-label={`Resize panes ${labelOf(column[r - 1] as PaneRef)} and ${labelOf(ref)}`}
                        aria-valuemin={0}
                        aria-valuemax={100}
                        aria-valuenow={Math.round(topShare(layout, i) * 100)}
                        tabIndex={0}
                        data-testid="pane-stack-divider"
                        onPointerDown={dragStack(i)}
                        onKeyDown={keyStack(i)}
                      >
                        <span className="absolute inset-x-0 -top-1.5 -bottom-1.5" />
                      </div>
                    )}
                    <section
                      className="flex min-h-0 min-w-0 flex-col"
                      style={{ flex: `${heightOf(layout, key)} 1 0`, opacity: multi && !focused ? 0.62 : 1, ...hide(closed) }}
                      data-pane={key}
                      data-focused={focused}
                      data-editor-pane={editor ? '' : undefined}
                      onMouseDown={() => focusAt(i)}
                    >
                      {isEditor(ref) && (
                        <EditorPane
                          session={s}
                          path={ref.path}
                          projectName={project?.name ?? ''}
                          focused={focused}
                          expanded={expandedKey === key}
                          paneKey={key}
                          dirty={dirty.has(key)}
                          onDirty={onDirty}
                          onToggleExpand={() => setExpanded(expandedKey === key ? null : key)}
                          onClose={() => close(i)}
                        />
                      )}
                      {!editor && multi && (
                        <div
                          className="flex h-7 shrink-0 items-center gap-2 border-b px-3 text-[12px]"
                          style={focused ? { boxShadow: 'inset 0 2px 0 var(--blue)' } : undefined}
                        >
                          {project && <ProjectTile project={project} />}
                          <span className="truncate">
                            {s.title} <span className="text-tdim">· {project?.name}</span>
                          </span>
                          <StatusGlyph status={s.status} />
                          <button
                            className="ml-auto grid h-5 w-5 place-items-center rounded-md text-dim hover:bg-sel"
                            aria-label="Close pane"
                            title="Close pane ⌘W"
                            onMouseDown={(e) => e.stopPropagation()}
                            onClick={(e) => {
                              e.stopPropagation();
                              close(i);
                            }}
                          >
                            <X size={12} aria-hidden="true" />
                          </button>
                        </div>
                      )}
                      {editor ? null : appearance.ready ? (
                        <TerminalView
                          sessionId={id}
                          focused={focused}
                          fontFamily={appearance.fontFamily}
                          fontSize={appearance.fontSize}
                          focusSignal={focusSignal}
                          label={`${s.title}, ${project?.name ?? AGENT_LABEL[s.agent]}`}
                        />
                      ) : (
                        <div className="min-h-0 flex-1 bg-term" />
                      )}
                    </section>
                  </Fragment>
                );
              })}
            </div>
          </Fragment>
        );
      })}
      {zone && (
        <div
          className={`pointer-events-none absolute z-10 grid place-items-center rounded-xl border-2 border-dashed border-blue text-[13px] text-blue ${zone.rect ? '' : 'inset-y-3 right-3 w-1/2'}`}
          style={{ ...zone.rect, background: 'color-mix(in srgb, var(--blue) 8%, transparent)' }}
          data-testid="drop-hint"
          data-zone={zone.direction}
        >
          {ZONE_LABEL[zone.direction]}
        </div>
      )}
      {notice && (
        <div
          key={notice.id}
          className="pointer-events-none absolute bottom-3 left-1/2 z-20 -translate-x-1/2 rounded-lg bg-txt px-3 py-1.5 text-[12px] text-win shadow-lg"
          role="status"
        >
          {notice.text}
        </div>
      )}
    </div>
  );
}
