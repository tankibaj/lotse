import { useCallback, useEffect, useRef, useState } from 'react';
import type { ErrorCode } from '../lib/client/types';
import { DaemonError } from '../lib/client/types';

export type Loaded<T> =
  | { status: 'loading' }
  | { status: 'error'; message: string; code: ErrorCode | null }
  | { status: 'ready'; data: T };

/**
 * Run `load` when the component mounts and whenever `reload()` is called; the answer of a stale call is dropped.
 * Read-only sheets (Show Changes, Show Transcript) use it: no polling, the user refreshes.
 */
export function useLoad<T>(load: () => Promise<T>): [Loaded<T>, () => void] {
  const [state, setState] = useState<Loaded<T>>({ status: 'loading' });
  const loadRef = useRef(load);
  /** Number of the latest call: an older answer is ignored. */
  const latest = useRef(0);
  useEffect(() => {
    loadRef.current = load;
  });

  const run = useCallback(() => {
    const call = ++latest.current;
    loadRef
      .current()
      .then((data) => call === latest.current && setState({ status: 'ready', data }))
      .catch((e: unknown) => {
        if (call !== latest.current) return;
        const code = e instanceof DaemonError ? e.code : null;
        setState({ status: 'error', code, message: e instanceof Error ? e.message : 'Something went wrong' });
      });
  }, []);

  /** Unmounted: drop the answer still on its way. */
  const drop = useCallback(() => {
    latest.current++;
  }, []);

  useEffect(() => {
    run();
    return drop;
  }, [run, drop]);

  const reload = useCallback(() => {
    setState((s) => (s.status === 'ready' ? s : { status: 'loading' })); // a refresh keeps what is shown
    run();
  }, [run]);

  return [state, reload];
}
