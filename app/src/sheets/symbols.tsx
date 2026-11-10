import { Folder } from 'lucide-react';
import { useEffect, useState } from 'react';
import { LUCIDE } from './lucideIcons';
import { loadSymbol, nativeSymbolsAvailable, peekSymbol, symbolScale } from './nativeSymbols';
import catalogue from './symbols.json';

/** The 15 project swatches (mock `SW`). `Project.color` stores the key. */
export const SWATCHES: Record<string, string> = {
  red: '#FF5E57', coral: '#FF7F50', orange: '#FF9F40', yellow: '#F5C518', green: '#4CB963',
  teal: '#2EC4B6', sky: '#7CC6F2', blue: '#3B82F6', indigo: '#5B6CD9', purple: '#9B59D0',
  lavender: '#B47EE5', pink: '#E87ACB', gray: '#8E8E93', sage: '#9BB59F', tan: '#C2A88A',
};

/**
 * One picker symbol. `id` is what `Project.icon` stores: a stable key (for the 41 symbols of the Lucide-only
 * picker, the Lucide id older versions saved, so saved icons keep working). `sf` is the SF Symbol the
 * app renders, `lucide` the stand-in for the browser build, `kw` extra search words.
 */
export interface SymbolEntry {
  id: string;
  sf: string;
  lucide: string;
  group: string;
  kw: string;
}

/** The curated symbol list (`symbols.json`, also read by the Rust test that renders every name). */
export const SYMBOLS: readonly SymbolEntry[] = catalogue;

const BY_KEY = new Map<string, SymbolEntry>();
for (const e of SYMBOLS) {
  BY_KEY.set(e.id, e);
  if (!BY_KEY.has(e.sf)) BY_KEY.set(e.sf, e);
}

/** The picker entry for a saved icon: by its id, or by SF Symbol name. */
export const symbolEntry = (key: string): SymbolEntry | undefined => BY_KEY.get(key);

/** Symbol groups in picker order. */
export const ICON_GROUPS: Record<string, string[]> = {};
for (const e of SYMBOLS) (ICON_GROUPS[e.group] ??= []).push(e.id);

/** Swatch colour for a key (unknown keys: gray). */
export const swatchColor = (key: string): string => SWATCHES[key] ?? '#8E8E93';

/** Symbols matching a search in their id, SF Symbol name, keywords or group. Empty groups are dropped. */
export function filterIcons(query: string): [string, string[]][] {
  const words = query.toLowerCase().split(/\s+/).filter(Boolean);
  const haystack = (e: SymbolEntry) => `${e.id} ${e.sf} ${e.lucide} ${e.kw} ${e.group}`.toLowerCase();
  return Object.entries(ICON_GROUPS)
    .map(([group, ids]): [string, string[]] => [group, ids.filter((id) => {
      const e = symbolEntry(id);
      return !!e && words.every((w) => haystack(e).includes(w));
    })])
    .filter(([, ids]) => ids.length > 0);
}

// A saved icon that is no catalogue id may still be an SF Symbol name (`hammer.fill`): the host tries it.
const SF_NAME = /^[a-z0-9]+(\.[a-z0-9]+)*$/;

/** The SF Symbol to ask the host for, for a saved icon key. */
const symbolName = (key: string): string | null => symbolEntry(key)?.sf ?? (key.includes('.') && SF_NAME.test(key) ? key : null);

/**
 * A project symbol, `size` px, in the current text colour. In the app it is the SF Symbol, drawn by
 * macOS and used as a CSS mask; in the browser build, or when the Mac has no such symbol, the
 * Lucide stand-in (the folder for a name nobody knows).
 */
export function ProjectIcon({ name, size = 20, strokeWidth = 1.8, outline = false }: { name: string; size?: number; strokeWidth?: number; outline?: boolean }) {
  // `outline`: the symbol without its `.fill` variant (when the Mac has no such symbol the Lucide outline stands in)
  const sfFull = nativeSymbolsAvailable() ? symbolName(name) : null;
  const sf = outline && sfFull ? sfFull.replace(/\.fill$/, '') : sfFull;
  const scale = symbolScale();
  // `undefined` until the host has answered, `null` when it has no such symbol
  const png = sf ? peekSymbol(sf, size, scale) : null;
  const [, answered] = useState(0);
  useEffect(() => {
    if (!sf || png !== undefined) return;
    let alive = true;
    void loadSymbol(sf, size, scale).then(() => alive && answered((n) => n + 1));
    return () => {
      alive = false;
    };
  }, [sf, png, size, scale]);

  if (sf && png) {
    const mask = `url(data:image/png;base64,${png})`;
    return (
      <span
        aria-hidden="true"
        data-symbol={sf}
        className="inline-block flex-none bg-current"
        style={{
          width: size,
          height: size,
          WebkitMaskImage: mask,
          maskImage: mask,
          WebkitMaskSize: 'contain',
          maskSize: 'contain',
          WebkitMaskRepeat: 'no-repeat',
          maskRepeat: 'no-repeat',
          WebkitMaskPosition: 'center',
          maskPosition: 'center',
        }}
      />
    );
  }
  // not asked yet: keep the space instead of flashing the Lucide shape
  if (sf && png === undefined) return <span aria-hidden="true" data-symbol-pending className="inline-block flex-none" style={{ width: size, height: size }} />;
  const Icon = LUCIDE[symbolEntry(name)?.lucide ?? ''] ?? Folder;
  return <Icon width={size} height={size} strokeWidth={strokeWidth} aria-hidden="true" />;
}
