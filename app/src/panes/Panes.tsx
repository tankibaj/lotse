import { Fragment, useCallback, useEffect, useRef, useState } from 'react';
import type { DragEvent, KeyboardEvent as ReactKeyboardEvent, PointerEvent as ReactPointerEvent } from 'react';
import { X } from 'lucide-react';
import type { Project, Session, SessionId } from '../lib/client';
import { onUiCommand } from '../lib/ui/commands';
import { selectSession, useSelectedSessionId } from '../lib/ui/selection';
import { TerminalView } from '../terminal';
import { useTerminalAppearance } from '../terminal/useTerminalAppearance';
import { ProjectTile } from './ProjectTile';
import { StatusGlyph } from './StatusGlyph';
import {
  MAX_PANES,
  addPane,
  closePane,
  emptyLayout,
  focusPane,
  focusStep,
  leftShare,
  nextSessionToOpen,
  resizePair,
  syncLayout,
  weightOf,
} from './layout';
import type { PaneLayout } from './layout';
import { onSplitRequest } from './splitBus';
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

/**
 * The terminal area: up to four sessions side by side with draggable dividers. The selected session
 * (`lib/ui/selection`) is the focused pane; focusing a pane selects its session.
 * Keys: ⌘D split, ⌘W close pane, ⌘[ ⌘] focus, ⌘+ ⌘− ⌘0 font size. ⌥-click on a session card, the card's
 * "Open in Split Right" and dropping a card here all end in `openInSplit` / `SESSION_DRAG_TYPE`.
 */
export function Panes() {
  const { sessions, projects, loaded } = useSessions();
  const selected = useSelectedSessionId();
  const appearance = useTerminalAppearance();
  const [layout, setLayout] = useState<PaneLayout>(emptyLayout);
  const [dropHint, setDropHint] = useState(false);
  const [notice, setNotice] = useState<Notice | null>(null);
  const [focusSignal, setFocusSignal] = useState(0);
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

  const say = useCallback((text: string) => setNotice((n) => ({ text, id: (n?.id ?? 0) + 1 })), []);

  const focusAt = useCallback(
    (index: number) => {
      const next = focusPane(layout, index);
      setLayout(next);
      const id = next.panes[next.focus];
      if (id !== undefined) selectSession(id);
    },
    [layout],
  );

  const split = useCallback(
    (wanted: SessionId | null) => {
      const id = wanted ?? nextSessionToOpen(sessions, layout.panes);
      if (id === null) return say('All sessions are already open');
      if (!sessions.some((s) => s.id === id)) return;
      const { layout: next, outcome } = addPane(layout, id);
      if (outcome === 'full') return say(`Up to ${MAX_PANES} panes side by side`);
      setLayout(next);
      selectSession(id);
    },
    [layout, sessions, say],
  );

  const close = useCallback(
    (index: number) => {
      const next = closePane(layout, index);
      if (next === layout) return layout.panes.length === 1 ? say('The last pane stays open') : undefined;
      setLayout(next);
      setFocusSignal((n) => n + 1); // the ✕ that had focus is gone: put the keyboard back in the terminal
      const id = next.panes[next.focus];
      if (id !== undefined) selectSession(id);
    },
    [layout, say],
  );

  const step = useCallback(
    (delta: number) => {
      const next = focusStep(layout, delta);
      if (next === layout) return;
      setLayout(next);
      const id = next.panes[next.focus];
      if (id !== undefined) selectSession(id);
    },
    [layout],
  );

  // Other WPs ask for a split through the bus (card context menu, palette).
  useEffect(() => onSplitRequest(split), [split]);
  useEffect(() => onUiCommand('splitSession', (id) => split(id ?? null)), [split]);

  // ⌥-click on a session card opens it side by side (cards carry `data-session-id`).
  useEffect(() => {
    const onClick = (e: MouseEvent) => {
      if (!e.altKey || !(e.target instanceof Element)) return;
      const id = e.target.closest('[data-session-id]')?.getAttribute('data-session-id');
      if (!id) return;
      e.preventDefault();
      e.stopPropagation();
      split(id);
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
      // elsewhere and "[" / "]" need ⌥, so `alt` is allowed. ⌘⇧D is the Changes panel, hence no shift for letters.
      const k = e.key.toLowerCase();
      if (!e.shiftKey && !e.altKey) {
        if (k === 'd') return handled(() => split(null));
        if (k === 'w') return handled(() => close(layout.focus));
        if (k === '-') return handled(() => appearance.zoom(-1));
        if (k === '0') return handled(appearance.resetZoom);
      }
      if (k === '[') return handled(() => step(-1));
      if (k === ']') return handled(() => step(1));
      // ⌘+ types "+" (⌘⇧= on a US keyboard); a bare "=" works too. ⌘⇧0 on a German layout types "=": not a zoom.
      if ((k === '=' && !e.shiftKey) || k === '+') return handled(() => appearance.zoom(1));
    };
    window.addEventListener('keydown', onKey);
    return () => window.removeEventListener('keydown', onKey);
  }, [split, close, step, layout.focus, appearance]);

  const dragDivider = (index: number) => (e: ReactPointerEvent<HTMLDivElement>) => {
    const els = box.current?.querySelectorAll<HTMLElement>('[data-pane]');
    const a = els?.[index - 1];
    const b = els?.[index];
    if (!a || !b) return;
    e.preventDefault();
    const x0 = e.clientX;
    const wa = a.getBoundingClientRect().width;
    const total = wa + b.getBoundingClientRect().width;
    document.body.style.userSelect = 'none';
    document.body.style.cursor = 'col-resize';
    const move = (ev: PointerEvent) => setLayout((l) => resizePair(l, index, wa + ev.clientX - x0, total));
    const up = () => {
      document.body.style.userSelect = '';
      document.body.style.cursor = '';
      window.removeEventListener('pointermove', move);
      window.removeEventListener('pointerup', up);
    };
    window.addEventListener('pointermove', move);
    window.addEventListener('pointerup', up);
  };

  const keyDivider = (index: number) => (e: ReactKeyboardEvent<HTMLDivElement>) => {
    if (e.key !== 'ArrowLeft' && e.key !== 'ArrowRight') return;
    const els = box.current?.querySelectorAll<HTMLElement>('[data-pane]');
    const a = els?.[index - 1];
    const b = els?.[index];
    if (!a || !b) return;
    e.preventDefault();
    const wa = a.getBoundingClientRect().width;
    const total = wa + b.getBoundingClientRect().width;
    setLayout((l) => resizePair(l, index, wa + (e.key === 'ArrowLeft' ? -KEY_RESIZE_PX : KEY_RESIZE_PX), total));
  };

  const isSessionDrag = (e: DragEvent) => [...e.dataTransfer.types].includes(SESSION_DRAG_TYPE);
  const onDragOver = (e: DragEvent) => {
    if (!isSessionDrag(e)) return;
    e.preventDefault();
    setDropHint(true);
  };
  const onDragLeave = (e: DragEvent) => {
    if (!e.currentTarget.contains(e.relatedTarget as Node | null)) setDropHint(false);
  };
  const onDrop = (e: DragEvent) => {
    setDropHint(false);
    const id = e.dataTransfer.getData(SESSION_DRAG_TYPE);
    if (!id) return;
    e.preventDefault();
    split(id);
  };

  const sessionById = (id: SessionId) => sessions.find((s) => s.id === id);
  const projectOf = (s: Session): Project | undefined => projects.find((p) => p.id === s.project_id);
  const multi = layout.panes.length > 1;

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
      {layout.panes.map((id, i) => {
        const s = sessionById(id);
        if (!s) return null;
        const project = projectOf(s);
        const focused = i === layout.focus;
        return (
          <Fragment key={id}>
            {i > 0 && (
              <div
                className="relative z-10 w-px shrink-0 cursor-col-resize bg-line focus-visible:bg-blue"
                role="separator"
                aria-orientation="vertical"
                aria-label={`Resize panes ${sessionById(layout.panes[i - 1] ?? '')?.title ?? ''} and ${s.title}`}
                aria-valuemin={0}
                aria-valuemax={100}
                aria-valuenow={Math.round(leftShare(layout, i) * 100)}
                tabIndex={0}
                data-testid="pane-divider"
                onPointerDown={dragDivider(i)}
                onKeyDown={keyDivider(i)}
              >
                <span className="absolute inset-y-0 -right-1.5 -left-1.5" />
              </div>
            )}
            <section
              className="flex min-w-0 flex-col"
              style={{ flex: `${weightOf(layout, id)} 1 0`, opacity: multi && !focused ? 0.62 : 1 }}
              data-pane={id}
              data-focused={focused}
              onMouseDown={() => focusAt(i)}
            >
              {multi && (
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
              {appearance.ready ? (
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
      {dropHint && (
        <div
          className="pointer-events-none absolute inset-y-3 right-3 z-10 grid w-1/2 place-items-center rounded-xl border-2 border-dashed border-blue text-[13px] text-blue"
          style={{ background: 'color-mix(in srgb, var(--blue) 8%, transparent)' }}
          data-testid="drop-hint"
        >
          Open side by side
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
