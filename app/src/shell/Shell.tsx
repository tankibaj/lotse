import { useCallback, useEffect, useRef, useState } from 'react';
import type { PointerEvent as ReactPointerEvent, ReactNode } from 'react';
import { Palette } from '../palette';
import { Panes, TabStrip } from '../panes';
import { SettingsPage } from '../settings';
import { Sheets } from '../sheets';
import { Sidebar } from '../sidebar';
import { useFullscreen } from './useFullscreen';
import { usePersistedFlag } from './usePersistedFlag';
import {
  SIDEBAR_SIDE_ITEM_ID,
  STATUSBAR_ITEM_ID,
  TOPBAR_LOCK_ITEM_ID,
  onMenuSidebarSide,
  onMenuStatusBar,
  onMenuTopBarLock,
} from '../settings/appMenu';
import { StatusBar } from '../statusbar';
import { Composer } from '../composer';
import { RightPanel } from '../panels';
import { Review } from '../review';
import { emitUiCommand, onUiCommand } from '../lib/ui/commands';
import { closeReview, getReview, isPanelName, openReview, togglePanel, useOpenPanel, useReview } from '../lib/ui/panel';
import type { PanelName } from '../lib/ui/panel';
import { getSelectedSessionId } from '../lib/ui/selection';

// Layout copied from spec/mock.html: sidebar 260 px (resizable 200-420, saved in `lotse-side`),
// 44 px title/tab rows, 28 px status bar. Each region renders a WP-owned component inside a
// shell-owned `data-slot` wrapper, so UI WPs fill their folder and never edit this file.

const SIDE_KEY = 'lotse-side';
const STATUSBAR_KEY = 'lotse-statusbar';
const TOPBAR_LOCK_KEY = 'lotse-topbar-lock';
const SIDE_RIGHT_KEY = 'lotse-side-right';
/** Full screen: how long the top bar stays after the pointer leaves it. */
const HIDE_DELAY_MS = 400;
/** Full screen in the app: the revealed bar sits below macOS's own title bar, which covers the top 32 px. */
const NATIVE_TITLEBAR = 'translate-y-8';
const SIDE_MIN = 200;
const SIDE_MAX = 420;
const SIDE_DEFAULT = 260;

const inTauri = () => typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window;

function loadWidth(): number {
  try {
    const w = Number(localStorage.getItem(SIDE_KEY));
    return w >= SIDE_MIN && w <= SIDE_MAX ? w : SIDE_DEFAULT;
  } catch {
    return SIDE_DEFAULT;
  }
}

/**
 * In the browser (dev, mock parity) draw the mock's dots; inside Tauri the native lights sit there
 * (`trafficLightPosition` in tauri.conf.json puts their centre on y = 22 px, x from 12 px, 52 px wide).
 */
function WindowControls() {
  if (inTauri()) return <span className="w-[52px] shrink-0" aria-hidden="true" />;
  return (
    <>
      <span className="h-3 w-3 rounded-full bg-[#FF5F57]" />
      <span className="h-3 w-3 rounded-full bg-[#FEBC2E]" />
      <span className="h-3 w-3 rounded-full bg-[#28C840]" />
    </>
  );
}

const iconButton =
  'grid h-[26px] w-7 place-items-center rounded-md text-dim hover:bg-sel';

/** The sidebar glyph of both the hide and the show button. */
const SidebarIcon = () => (
  <svg width="16" height="16" fill="none" stroke="currentColor" strokeWidth="1.5" aria-hidden="true">
    <rect x="1.5" y="2.5" width="13" height="11" rx="2" />
    <path d="M6 2.5v11" />
  </svg>
);

// The mock's panel toolbar (right end of the tab row): one panel at a time, each button toggles its panel (`lib/ui/panel`).
// `code` is the physical key of the ⇧⌘ shortcut, so it works on any keyboard layout.
const PANELS: { id: PanelName; name: string; keys: string; code: string; paths: string; rect?: boolean }[] = [
  { id: 'session', name: 'Session', keys: '⇧⌘I', code: 'KeyI', paths: 'M5 6h6M5 8.5h6M5 11h4', rect: true },
  { id: 'changes', name: 'Changes', keys: '⇧⌘G', code: 'KeyG', paths: 'M8 4.5v4M6 6.5h4M6 11h4', rect: true },
  { id: 'files', name: 'Files', keys: '⇧⌘F', code: 'KeyF', paths: 'M2 4.5a1.5 1.5 0 0 1 1.5-1.5H6l1.5 1.5h5A1.5 1.5 0 0 1 14 6v5.5a1.5 1.5 0 0 1-1.5 1.5h-9A1.5 1.5 0 0 1 2 11.5z' },
  { id: 'transcript', name: 'Transcript', keys: '⇧⌘T', code: 'KeyT', paths: 'M3 3.5h10M3 6.5h7M3 9.5h10M3 12.5h5' },
  { id: 'usage', name: 'Usage & time', keys: '⇧⌘U', code: 'KeyU', paths: 'M3 13V8M6.5 13V4M10 13V9M13.5 13V6' },
];

function PanelToolbar() {
  const open = useOpenPanel();
  return (
    <div className="ml-auto flex shrink-0 items-center gap-0.5" role="toolbar" aria-label="Panels">
      {PANELS.map((p) => (
        <button
          key={p.name}
          type="button"
          aria-pressed={open === p.id}
          aria-label={p.name === 'Usage & time' ? 'Usage and time' : p.name}
          title={`${p.name} ${p.keys}`}
          className={`grid h-[26px] w-7 place-items-center rounded-md hover:bg-sel ${open === p.id ? 'bg-sel text-txt' : 'text-dim'}`}
          onClick={() => emitUiCommand('togglePanel', p.id)}
        >
          <svg width="16" height="16" fill="none" stroke="currentColor" strokeWidth="1.4" aria-hidden="true">
            {p.rect && <rect x="2" y="2" width="12" height="12" rx="2.5" />}
            <path d={p.paths} />
          </svg>
        </button>
      ))}
    </div>
  );
}

/**
 * Full screen: a thin strip at the top edge that shows the bar (rendered by `children(peek)`) while the pointer is
 * over it or focus is inside. Its state dies with it, so a hover cut short by an unmount cannot linger.
 */
function TopBarZone({ children }: { children: (peek: boolean) => ReactNode }) {
  const [hover, setHover] = useState(false);
  const [focusIn, setFocusIn] = useState(false);
  // macOS slides its own title bar (and menu bar) over the window's top edge, which takes the pointer from the
  // webview for a moment: the bar waits a little before it hides, so the pointer can cross it to reach the bar.
  const leaveTimer = useRef<number>(0);
  useEffect(() => () => window.clearTimeout(leaveTimer.current), []);
  const enter = () => {
    window.clearTimeout(leaveTimer.current);
    setHover(true);
  };
  const leave = () => {
    window.clearTimeout(leaveTimer.current);
    leaveTimer.current = window.setTimeout(() => setHover(false), HIDE_DELAY_MS);
  };
  return (
    <div
      className="relative z-30 h-1.5 shrink-0"
      data-testid="topbar-hover"
      onMouseEnter={enter}
      onMouseLeave={leave}
      onFocusCapture={() => setFocusIn(true)}
      onBlurCapture={() => setFocusIn(false)}
    >
      {children(hover || focusIn)}
    </div>
  );
}

export function Shell() {
  const [width, setWidth] = useState(loadWidth);
  const [hidden, setHidden] = useState(false);

  const toggleSidebar = useCallback(() => setHidden((h) => !h), []);
  // View menu choices, saved in localStorage and mirrored as menu checkmarks. The footer status bar starts hidden.
  // On the right the native traffic lights sit over the main column's bar. Full screen hides the top bar until the
  // pointer reaches the top edge, unless it is locked.
  const [statusBar, toggleStatusBar] = usePersistedFlag(STATUSBAR_KEY, STATUSBAR_ITEM_ID);
  const [sideRight, toggleSideRight] = usePersistedFlag(SIDE_RIGHT_KEY, SIDEBAR_SIDE_ITEM_ID);
  const [topBarLocked, toggleTopBarLock] = usePersistedFlag(TOPBAR_LOCK_KEY, TOPBAR_LOCK_ITEM_ID);
  useEffect(() => onMenuStatusBar(toggleStatusBar), [toggleStatusBar]);
  useEffect(() => onMenuSidebarSide(toggleSideRight), [toggleSideRight]);
  useEffect(() => onMenuTopBarLock(toggleTopBarLock), [toggleTopBarLock]);
  const lightsInMain = hidden || sideRight;
  const isFullscreen = useFullscreen(); // macOS draws no traffic lights in full screen
  const fullscreen = isFullscreen && !topBarLocked;
  const review = useReview();

  useEffect(() => onUiCommand('toggleSidebar', toggleSidebar), [toggleSidebar]);
  useEffect(() => onUiCommand('togglePanel', (name) => isPanelName(name) && togglePanel(name)), []);
  useEffect(() => onUiCommand('openReview', (sessionId, file) => sessionId && openReview(sessionId, file)), []);
  // The Tauri window is transparent with the macOS sidebar material behind it (tauri.conf.json); polish.css lets it
  // show through the sidebar and status bar. In a browser there is no material, so the solid mock colours stay.
  useEffect(() => {
    if (!inTauri()) return;
    document.documentElement.dataset.vibrancy = '';
    return () => document.documentElement.removeAttribute('data-vibrancy');
  }, []);
  // Esc leaves the review. Capture phase: xterm stops the keydown of a focused terminal from bubbling up to the window.
  useEffect(() => {
    const onEsc = (e: KeyboardEvent) => {
      if (e.key !== 'Escape' || !getReview()) return;
      e.preventDefault();
      e.stopPropagation(); // the terminal behind must not also receive the Esc
      closeReview();
    };
    addEventListener('keydown', onEsc, true);
    return () => removeEventListener('keydown', onEsc, true);
  }, []);
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (!e.metaKey) return;
      const handled = (fn: () => void) => {
        e.preventDefault();
        fn();
      };
      if (e.ctrlKey) return e.code === 'KeyS' ? handled(toggleSidebar) : undefined;
      // ⇧⌘ + letter: the panels. ⌘⇧Return: the composer. ⇧⌘R: review the selected session's changes
      // (⌥⌘D is macOS's own Show/Hide Dock: the OS takes it before the app sees it).
      if (e.shiftKey && !e.altKey) {
        const panel = PANELS.find((p) => p.code === e.code);
        if (panel) return handled(() => emitUiCommand('togglePanel', panel.id));
        if (e.code === 'Enter') return handled(() => emitUiCommand('openComposer'));
        if (e.code === 'KeyR') {
          const id = getSelectedSessionId();
          if (id) handled(() => emitUiCommand('openReview', id));
        }
      }
    };
    addEventListener('keydown', onKey);
    return () => removeEventListener('keydown', onKey);
  }, [toggleSidebar]);

  const startResize = (e: ReactPointerEvent<HTMLDivElement>) => {
    e.preventDefault();
    const x0 = e.clientX;
    const w0 = width;
    let latest = w0;
    document.body.style.userSelect = 'none';
    document.body.style.cursor = 'col-resize';
    const move = (ev: PointerEvent) => {
      latest = Math.min(SIDE_MAX, Math.max(SIDE_MIN, w0 + (sideRight ? x0 - ev.clientX : ev.clientX - x0)));
      setWidth(latest);
    };
    const up = () => {
      document.body.style.userSelect = '';
      document.body.style.cursor = '';
      removeEventListener('pointermove', move);
      removeEventListener('pointerup', up);
      try {
        localStorage.setItem(SIDE_KEY, String(latest));
      } catch {
        // storage unavailable
      }
    };
    addEventListener('pointermove', move);
    addEventListener('pointerup', up);
  };

  // The button that brings the sidebar back sits on the side the sidebar belongs to.
  const showSidebarButton = (extra: string) => (
    <button className={`${iconButton} ${extra}`} aria-label="Show sidebar" title="Show sidebar ⌃⌘S" onClick={toggleSidebar}>
      <SidebarIcon />
    </button>
  );

  const renderTopBar = (peek: boolean) => (
    <div
      className={`flex h-11 items-center border-b pt-px ${lightsInMain ? 'gap-2 px-3' : 'gap-1 px-2'} ${
        fullscreen ? 'absolute inset-x-0 top-0 bg-win shadow-lg transition-transform duration-150' : 'shrink-0'
      } ${fullscreen ? (peek ? (inTauri() ? NATIVE_TITLEBAR : '') : '-translate-y-full') : ''}`}
      data-tauri-drag-region="deep"
    >
      {lightsInMain && (
        <>
          {!isFullscreen && <WindowControls />}
          {hidden && !sideRight && showSidebarButton(isFullscreen ? '' : 'ml-2')}
        </>
      )}
      <div data-slot="tabs" className="flex min-w-0 flex-1 items-center gap-1 self-stretch">
        <TabStrip />
      </div>
      <PanelToolbar />
      {hidden && sideRight && showSidebarButton('')}
    </div>
  );

  return (
    <div className="relative flex h-screen min-h-[560px] flex-col bg-win text-txt" data-testid="shell">
      <div className={`flex min-h-0 flex-1 ${sideRight ? 'flex-row-reverse' : ''}`}>
        {!hidden && (
          <>
            <aside
              className={`flex min-h-0 min-w-0 shrink-0 flex-col overflow-hidden ${sideRight ? 'border-l' : 'border-r'} bg-side lotse-glass`}
              style={{ width }}
              aria-label="Sidebar"
              data-testid="sidebar-region"
            >
              <div className="flex h-11 shrink-0 items-center gap-2 px-3" data-tauri-drag-region="deep">
                {sideRight ? <span className="w-0" /> : <WindowControls />}
                <span className={sideRight ? 'font-semibold' : 'ml-2 font-semibold'}>Lotse</span>
                <button
                  className={`${iconButton} ml-auto`}
                  aria-label="Settings"
                  title="Settings ⌘,"
                  onClick={() => emitUiCommand('openSettings')}
                >
                  <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.6" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
                    <path d="M12.22 2h-.44a2 2 0 0 0-2 2v.18a2 2 0 0 1-1 1.73l-.43.25a2 2 0 0 1-2 0l-.15-.08a2 2 0 0 0-2.73.73l-.22.38a2 2 0 0 0 .73 2.73l.15.1a2 2 0 0 1 1 1.72v.51a2 2 0 0 1-1 1.74l-.15.09a2 2 0 0 0-.73 2.73l.22.38a2 2 0 0 0 2.73.73l.15-.08a2 2 0 0 1 2 0l.43.25a2 2 0 0 1 1 1.73V20a2 2 0 0 0 2 2h.44a2 2 0 0 0 2-2v-.18a2 2 0 0 1 1-1.73l.43-.25a2 2 0 0 1 2 0l.15.08a2 2 0 0 0 2.73-.73l.22-.39a2 2 0 0 0-.73-2.73l-.15-.08a2 2 0 0 1-1-1.74v-.5a2 2 0 0 1 1-1.74l.15-.09a2 2 0 0 0 .73-2.73l-.22-.38a2 2 0 0 0-2.73-.73l-.15.08a2 2 0 0 1-2 0l-.43-.25a2 2 0 0 1-1-1.73V4a2 2 0 0 0-2-2z" />
                    <circle cx="12" cy="12" r="3" />
                  </svg>
                </button>
                <button
                  className={iconButton}
                  aria-label="Hide sidebar (⌃⌘S)"
                  title="Hide sidebar ⌃⌘S"
                  onClick={toggleSidebar}
                >
                  <SidebarIcon />
                </button>
              </div>
              <div data-slot="sidebar" className="flex min-h-0 flex-1 flex-col">
                <Sidebar />
              </div>
            </aside>
            <div
              className={`z-10 w-1 cursor-col-resize hover:bg-blue/40 ${sideRight ? '-mr-0.5' : '-ml-0.5'}`}
              role="separator"
              aria-label="Resize sidebar"
              aria-orientation="vertical"
              onPointerDown={startResize}
            />
          </>
        )}
        <main className="relative flex min-w-0 flex-1 flex-col">
          {/* Everything in this row is centred on y = 22 px, the traffic lights' centre line (`pt-px` offsets the 1 px
              border). Sidebar hidden: the lights sit over the row's start, so it starts like the sidebar header
              (controls or their spacer, then the toggle where the title would begin). Tabs stay bottom-attached. */}
          {/* Full screen: the bar leaves the layout and slides in over the content while the pointer is at the top edge
              (or focus is inside it). */}
          {fullscreen ? (
            <TopBarZone>{renderTopBar}</TopBarZone>
          ) : (
            renderTopBar(true)
          )}
          <div className="flex min-h-0 flex-1">
            {/* A column: the panes take what the composer leaves, so an open composer shrinks (and refits) the terminals
                instead of covering the bottom of them. */}
            <div data-slot="panes" className="relative flex min-h-0 min-w-0 flex-1 flex-col">
              <div className="flex min-h-0 min-w-0 flex-1">
                <Panes />
              </div>
              <div data-slot="composer" className="contents">
                <Composer />
              </div>
            </div>
            <div data-slot="panel" className="contents">
              <RightPanel />
            </div>
          </div>
        </main>
      </div>
      {/* Hidden, not unmounted: the status bar also owns the ⌘T / ⌥⌘T session shortcuts and the daemon retry. */}
      <div data-slot="statusbar" className={statusBar ? 'contents' : 'hidden'}>
        <StatusBar />
      </div>
      <div data-slot="sheets" className="contents">
        <Sheets />
      </div>
      <div data-slot="review" className="contents">
        {review && <Review sessionId={review.sessionId} file={review.file} onClose={closeReview} />}
      </div>
      <div data-slot="settings" className="contents">
        <SettingsPage />
      </div>
      <div data-slot="palette" className="contents">
        <Palette />
      </div>
    </div>
  );
}
