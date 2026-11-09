import { useEffect, useRef } from 'react';
import type { Display } from './layout';

const inTauri = () => typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window;

const browserScreen = (): Display => ({ name: 'screen', width: window.screen.width, height: window.screen.height });

/** The display the window is on, in logical points: Tauri's current monitor, else the browser's screen. */
export async function currentDisplay(): Promise<Display> {
  if (!inTauri()) return browserScreen();
  try {
    const { currentMonitor } = await import('@tauri-apps/api/window');
    const monitor = await currentMonitor();
    if (monitor === null) return browserScreen();
    const size = monitor.size.toLogical(monitor.scaleFactor);
    return { name: monitor.name ?? 'display', width: Math.round(size.width), height: Math.round(size.height) };
  } catch {
    return browserScreen();
  }
}

const same = (a: Display | null, b: Display) => a !== null && a.name === b.name && a.width === b.width && a.height === b.height;

/**
 * Calls `onChange` with the display the window is on, once it is known and again whenever it differs. Re-read when the
 * window resizes or regains focus, which is when it has been moved to another display.
 */
export function useDisplayChange(onChange: (d: Display) => void): void {
  const latest = useRef(onChange);
  useEffect(() => {
    latest.current = onChange;
  });
  useEffect(() => {
    let live = true;
    let last: Display | null = null;
    const refresh = () =>
      void currentDisplay().then((d) => {
        if (!live || same(last, d)) return;
        last = d;
        latest.current(d);
      });
    refresh();
    window.addEventListener('resize', refresh);
    window.addEventListener('focus', refresh);
    return () => {
      live = false;
      window.removeEventListener('resize', refresh);
      window.removeEventListener('focus', refresh);
    };
  }, []);
}
