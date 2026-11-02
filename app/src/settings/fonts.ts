import { useEffect, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { inTauri } from '../sheets/hostBridge';

/** What `list_fonts` (`app/src-tauri/src/fonts.rs`) returns: sorted, deduped; `monospace` ⊆ `families`. */
export interface InstalledFonts {
  families: string[];
  monospace: string[];
}

/** The browser/Mock build has no font list: the short lists the pickers used before. */
export const FALLBACK_FONTS: InstalledFonts = {
  families: ['Helvetica Neue', 'Inter', 'JetBrains Mono', 'Menlo', 'SF Mono'],
  monospace: ['JetBrains Mono', 'Menlo', 'SF Mono'],
};

let source: (() => Promise<InstalledFonts>) | null = null;
let cached: Promise<InstalledFonts> | null = null;

/** Replace the font source (tests). `null` restores the default. */
export function setFontSource(s: (() => Promise<InstalledFonts>) | null): void {
  source = s;
  cached = null;
}

/** Forget the loaded list: the next `loadInstalledFonts` asks again (once per Settings open). */
export function resetInstalledFonts(): void {
  cached = null;
}

const fetchFonts = (): Promise<InstalledFonts> => {
  if (source) return source();
  if (!inTauri()) return Promise.resolve(FALLBACK_FONTS);
  return invoke<InstalledFonts>('list_fonts');
};

/** Installed fonts, asked once until `resetInstalledFonts`. A failed lookup falls back to the short lists. */
export function loadInstalledFonts(): Promise<InstalledFonts> {
  cached ??= fetchFonts().catch(() => FALLBACK_FONTS);
  return cached;
}

/** The installed fonts; the short fallback lists until the lookup answers (or when it fails). */
export function useInstalledFonts(): InstalledFonts {
  const [fonts, setFonts] = useState<InstalledFonts>(FALLBACK_FONTS);
  useEffect(() => {
    let alive = true;
    void loadInstalledFonts().then((f) => alive && setFonts(f));
    return () => {
      alive = false;
    };
  }, []);
  return fonts;
}
