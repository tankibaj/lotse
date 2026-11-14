import { useEffect, useRef, useState } from 'react';

/**
 * "Now" for coarse time labels. Checks every `tickMs`, but only changes (and so re-renders) when the
 * visible text would change (`signature(now)`), and never while `hold()` is true (pointer or focus is
 * in the sidebar), so hover states and buttons do not flicker. It catches up on the first tick after.
 * The timer is stable: new `signature` / `hold` functions are picked up without restarting it, so a
 * busy event stream cannot starve it.
 */
export function useCoarseNow(signature: (now: number) => string, hold: () => boolean, tickMs = 1000): number {
  const [now, setNow] = useState(() => Date.now());
  const latest = useRef({ signature, hold });
  useEffect(() => {
    latest.current = { signature, hold };
  });
  useEffect(() => {
    const id = setInterval(() => {
      const { signature: sig, hold: held } = latest.current;
      if (held()) return;
      const t = Date.now();
      setNow((prev) => (sig(prev) === sig(t) ? prev : t));
    }, tickMs);
    return () => clearInterval(id);
  }, [tickMs]);
  return now;
}
