import { useEffect, useRef } from 'react';
import { DISPLAY_FALLBACK_NAME } from './layout';
import type { Display } from './layout';

const inTauri = () => typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window;

const browserScreen = (): Display => ({ name: DISPLAY_FALLBACK_NAME, width: window.screen.width, height: window.screen.height });

/** The display the window is on, in logical points: Tauri's current monitor, else the browser's screen. */
export async function currentDisplay(): Promise<Display> {
  if (!inTauri()) return browserScreen();
  try {
    const { currentMonitor } = await import('@tauri-apps/api/window');
    const monitor = await currentMonitor();
    if (monitor === null) return browserScreen();
    const size = monitor.size.toLogical(monitor.scaleFactor);
    const at = monitor.position.toLogical(monitor.scaleFactor);
    return {
      name: monitor.name ?? DISPLAY_FALLBACK_NAME,
      width: Math.round(size.width),
      height: Math.round(size.height),
      scale: monitor.scaleFactor,
      x: Math.round(at.x),
      y: Math.round(at.y),
    };
  } catch {
    return browserScreen();
  }
}

/** A resize burst (dragging the window edge) is looked up once, this long after it ends. */
export const DISPLAY_DEBOUNCE_MS = 200;

const same = (a: Display | null, b: Display) =>
  a !== null && a.name === b.name && a.width === b.width && a.height === b.height && a.scale === b.scale && a.x === b.x && a.y === b.y;

/**
 * Calls `onChange` with the display the window is on, once it is known and again whenever it differs. Re-read when the
 * window regains focus, and (debounced) when it resizes: that is when it has been moved to another display.
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
    let timer: number | undefined;
    const refreshSoon = () => {
      window.clearTimeout(timer);
      timer = window.setTimeout(refresh, DISPLAY_DEBOUNCE_MS);
    };
    refresh();
    window.addEventListener('resize', refreshSoon);
    window.addEventListener('focus', refresh);
    return () => {
      live = false;
      window.clearTimeout(timer);
      window.removeEventListener('resize', refreshSoon);
      window.removeEventListener('focus', refresh);
    };
  }, []);
}
