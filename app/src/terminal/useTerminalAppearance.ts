import { useCallback, useEffect, useRef, useState } from 'react';
import { useClient } from '../lib/client';
import type { Settings } from '../lib/client';
import { DEFAULT_FONT_SIZE, clampFontSize, terminalFontFamily } from './font';

export interface TerminalAppearance {
  /** Settings arrived (or could not be read). Terminals wait for it so they open at their final font instead of changing it right after. */
  ready: boolean;
  fontFamily: string;
  fontSize: number;
  /** ⌘+ (delta 1) and ⌘− (delta -1). Saved through `updateSettings`, so Settings shows the same size. */
  zoom: (delta: number) => void;
  /** ⌘0 */
  resetZoom: () => void;
}

/** Terminal font family and size from the daemon's settings (`appearance.terminal_font*`), kept live through `settings_changed`. */
export function useTerminalAppearance(): TerminalAppearance {
  const client = useClient();
  const [settings, setSettings] = useState<Settings | null>(null);
  const [failed, setFailed] = useState(false);
  const latest = useRef<Settings | null>(null);

  useEffect(() => {
    let alive = true;
    const apply = (s: Settings) => {
      latest.current = s;
      setSettings(s);
      setFailed(false);
    };
    // A failed read must not leave the terminals blank: they open with the default font, and the next try fixes it.
    const load = () =>
      client.getSettings().then(
        (s) => alive && apply(s),
        () => alive && setFailed(true),
      );
    void load();
    const off = client.onEvent((e) => {
      if (e.event === 'settings_changed') apply(e.data);
    });
    const offConn = client.onConnection((state) => {
      if (state === 'reconnected') void load();
    });
    return () => {
      alive = false;
      off();
      offConn();
    };
  }, [client]);

  const setSize = useCallback(
    (size: number) => {
      const cur = latest.current;
      if (!cur) return;
      const next = clampFontSize(size);
      if (next === cur.appearance.terminal_font_size) return;
      const updated: Settings = { ...cur, appearance: { ...cur.appearance, terminal_font_size: next } };
      latest.current = updated;
      setSettings(updated);
      client.updateSettings(updated).catch(() => {
        // daemon refused: the next settings_changed event restores the saved value
      });
    },
    [client],
  );

  const zoom = useCallback((delta: number) => setSize((latest.current?.appearance.terminal_font_size ?? DEFAULT_FONT_SIZE) + delta), [setSize]);
  const resetZoom = useCallback(() => setSize(DEFAULT_FONT_SIZE), [setSize]);

  return {
    ready: settings !== null || failed,
    fontFamily: terminalFontFamily(settings?.appearance.terminal_font),
    fontSize: clampFontSize(settings?.appearance.terminal_font_size ?? DEFAULT_FONT_SIZE),
    zoom,
    resetZoom,
  };
}
