import { useCallback, useEffect, useState } from 'react';
import type { PointerEvent as ReactPointerEvent } from 'react';
import { Palette } from '../palette';
import { Panes, TabStrip } from '../panes';
import { SettingsPage } from '../settings';
import { Sheets } from '../sheets';
import { Sidebar } from '../sidebar';
import { StatusBar } from '../statusbar';
import { emitUiCommand, onUiCommand } from '../lib/ui/commands';

// Layout copied from spec/mock.html: sidebar 260 px (resizable 200-420, saved in `lotse-side`),
// 44 px title/tab rows, 28 px status bar. Each region renders a WP-owned component inside a
// shell-owned `data-slot` wrapper, so UI WPs fill their folder and never edit this file.

const SIDE_KEY = 'lotse-side';
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

/** In the browser (dev, mock parity) draw the mock's dots; inside Tauri the native lights sit there. */
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

export function Shell() {
  const [width, setWidth] = useState(loadWidth);
  const [hidden, setHidden] = useState(false);

  const toggleSidebar = useCallback(() => setHidden((h) => !h), []);

  useEffect(() => onUiCommand('toggleSidebar', toggleSidebar), [toggleSidebar]);
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.metaKey && e.ctrlKey && e.code === 'KeyS') {
        e.preventDefault();
        toggleSidebar();
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
      latest = Math.min(SIDE_MAX, Math.max(SIDE_MIN, w0 + ev.clientX - x0));
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

  return (
    <div className="relative flex h-screen min-h-[560px] flex-col bg-win text-txt" data-testid="shell">
      <div className="flex min-h-0 flex-1">
        {!hidden && (
          <>
            <aside
              className="flex min-h-0 min-w-0 shrink-0 flex-col overflow-hidden border-r bg-side lotse-glass"
              style={{ width }}
              aria-label="Sidebar"
              data-testid="sidebar-region"
            >
              <div className="flex h-11 shrink-0 items-center gap-2 px-3" data-tauri-drag-region="deep">
                <WindowControls />
                <span className="ml-2 font-semibold">Lotse</span>
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
                  <svg width="16" height="16" fill="none" stroke="currentColor" strokeWidth="1.5" aria-hidden="true">
                    <rect x="1.5" y="2.5" width="13" height="11" rx="2" />
                    <path d="M6 2.5v11" />
                  </svg>
                </button>
              </div>
              <div data-slot="sidebar" className="flex min-h-0 flex-1 flex-col">
                <Sidebar />
              </div>
            </aside>
            <div
              className="z-10 -ml-0.5 w-1 cursor-col-resize hover:bg-blue/40"
              role="separator"
              aria-label="Resize sidebar"
              aria-orientation="vertical"
              onPointerDown={startResize}
            />
          </>
        )}
        <main className="flex min-w-0 flex-1 flex-col">
          <div className="flex h-11 shrink-0 items-end gap-1 border-b px-2" data-tauri-drag-region="deep">
            {hidden && (
              <button className={`${iconButton} mb-2`} aria-label="Show sidebar" onClick={toggleSidebar}>
                ☰
              </button>
            )}
            <div data-slot="tabs" className="flex min-w-0 flex-1 items-end gap-1">
              <TabStrip />
            </div>
          </div>
          <div className="flex min-h-0 flex-1">
            <div data-slot="panes" className="relative flex min-w-0 flex-1">
              <Panes />
            </div>
          </div>
        </main>
      </div>
      <div data-slot="statusbar" className="contents">
        <StatusBar />
      </div>
      <div data-slot="sheets" className="contents">
        <Sheets />
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
