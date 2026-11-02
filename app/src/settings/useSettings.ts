import { useCallback, useEffect, useRef, useState } from 'react';
import { useClient } from '../lib/client';
import type { LaunchCommandInfo, Settings } from '../lib/client/types';

export interface SettingsDraft {
  settings: Settings | null;
  launch: LaunchCommandInfo[];
  error: string | null;
  /** Change a copy of the settings; saved at once, or after `delayMs` (typing). Pending saves flush on unmount. */
  update(mutate: (s: Settings) => void, delayMs?: number): void;
  /** Save a pending change now. */
  flush(): void;
}

/**
 * Settings page state: loads settings and detected launch commands, writes the whole object back
 * (`update_settings` replaces it). Our own writes are not re-read, so typing is never overwritten.
 */
export function useSettings(): SettingsDraft {
  const client = useClient();
  const [settings, setSettings] = useState<Settings | null>(null);
  const [launch, setLaunch] = useState<LaunchCommandInfo[]>([]);
  const [error, setError] = useState<string | null>(null);
  const latest = useRef<Settings | null>(null);
  const timer = useRef<ReturnType<typeof setTimeout> | null>(null);

  useEffect(() => {
    let alive = true;
    const load = () => {
      client
        .getSettings()
        .then((s) => {
          if (!alive) return;
          latest.current = s;
          setSettings(s);
        })
        .catch((e: unknown) => alive && setError(e instanceof Error ? e.message : 'Could not load settings'));
      client
        .detectLaunchCommands()
        .then((l) => alive && setLaunch(l))
        .catch(() => {});
    };
    load();
    const off = client.onConnection((s) => s === 'reconnected' && timer.current === null && load());
    return () => {
      alive = false;
      off();
    };
  }, [client]);

  const flush = useCallback(() => {
    if (timer.current !== null) {
      clearTimeout(timer.current);
      timer.current = null;
    }
    const s = latest.current;
    if (s) client.updateSettings(s).then(() => setError(null), (e: unknown) => setError(e instanceof Error ? e.message : 'Could not save'));
  }, [client]);

  const update = useCallback(
    (mutate: (s: Settings) => void, delayMs = 0) => {
      const base = latest.current;
      if (!base) return;
      const next = structuredClone(base);
      mutate(next);
      latest.current = next;
      setSettings(next);
      if (timer.current !== null) clearTimeout(timer.current);
      if (delayMs > 0) timer.current = setTimeout(flush, delayMs);
      else flush();
    },
    [flush],
  );

  // Leaving the page (or switching section) must not lose a change still waiting for its timer.
  useEffect(
    () => () => {
      if (timer.current !== null) flush();
    },
    [flush],
  );

  return { settings, launch, error, update, flush };
}
