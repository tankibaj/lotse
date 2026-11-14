import { useEffect, useState } from 'react';
import { inTauri } from '../sheets/hostBridge';

/** True while the window is full screen: the native window in the app, the document in a browser. */
export function useFullscreen(): boolean {
  const [full, setFull] = useState(false);
  useEffect(() => {
    if (!inTauri()) {
      const sync = () => setFull(!!document.fullscreenElement);
      sync();
      document.addEventListener('fullscreenchange', sync);
      return () => document.removeEventListener('fullscreenchange', sync);
    }
    let stopped = false;
    let off: (() => void) | undefined;
    void import('@tauri-apps/api/window').then(async ({ getCurrentWindow }) => {
      const win = getCurrentWindow();
      let seq = 0; // resizes come in bursts: only the newest answer counts
      const sync = () => {
        const mine = ++seq;
        void win.isFullscreen().then((f) => !stopped && mine === seq && setFull(f)).catch(() => {});
      };
      sync();
      const un = await win.onResized(sync); // entering and leaving full screen resize the window
      if (stopped) un();
      else off = un;
    }).catch(() => {}); // no window API (tests, odd hosts): never full screen
    return () => {
      stopped = true;
      off?.();
    };
  }, []);
  return full;
}
