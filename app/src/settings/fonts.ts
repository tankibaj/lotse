import { useEffect, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { inTauri } from '../sheets/hostBridge';

/** What `list_fonts` (`app/src-tauri/src/fonts.rs`) returns: sorted, deduped; `monospace` ⊆ `families`. */
export interface InstalledFonts {
  families: string[];
  monospace: string[];
}

/** The installed fonts and whether they were really read from this Mac (`false`: the short built-in lists). */
export interface FontList extends InstalledFonts {
  verified: boolean;
}

/** The browser/Mock build has no font list: the short lists the pickers used before. */
export const FALLBACK_FONTS: InstalledFonts = {
  families: ['Helvetica Neue', 'Inter', 'JetBrains Mono', 'Menlo', 'SF Mono'],
  monospace: ['JetBrains Mono', 'Menlo', 'SF Mono'],
};

const FALLBACK_LIST: FontList = { ...FALLBACK_FONTS, verified: false };

let source: (() => Promise<InstalledFonts>) | null = null;
let cached: Promise<FontList> | null = null;

/** Replace the font source (tests). `null` restores the default. */
export function setFontSource(s: (() => Promise<InstalledFonts>) | null): void {
  source = s;
  cached = null;
}

/** Forget the loaded list: the next `loadInstalledFonts` asks again (once per Settings open). */
export function resetInstalledFonts(): void {
  cached = null;
}

const fetchFonts = (): Promise<FontList> => {
  const read = source ?? (inTauri() ? () => invoke<InstalledFonts>('list_fonts') : null);
  if (!read) return Promise.resolve(FALLBACK_LIST); // browser build: nothing to ask
  // A failed lookup is an unknown, not an empty Mac: the short lists, flagged as not read.
  return read().then(
    (fonts) => ({ ...fonts, verified: true }),
    () => FALLBACK_LIST,
  );
};

const loadFontList = (): Promise<FontList> => (cached ??= fetchFonts());

/** Installed fonts, asked once until `resetInstalledFonts`. A failed lookup falls back to the short lists. */
export function loadInstalledFonts(): Promise<InstalledFonts> {
  return loadFontList().then(({ families, monospace }) => ({ families, monospace }));
}

/**
 * The installed fonts; the short fallback lists until the lookup answers (or when it fails). `verified` turns true
 * only with a real answer: until then a stored font missing from the short lists is not known to be missing.
 */
export function useInstalledFonts(): FontList {
  const [fonts, setFonts] = useState<FontList>(FALLBACK_LIST);
  useEffect(() => {
    let alive = true;
    void loadFontList().then((f) => alive && setFonts(f));
    return () => {
      alive = false;
    };
  }, []);
  return fonts;
}
