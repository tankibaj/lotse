import { invoke } from '@tauri-apps/api/core';
import { inTauri } from './hostBridge';

// SF Symbols from the Tauri host (`symbol_png`, app/src-tauri/src/symbols.rs): a base64 PNG, black on
// transparent, used as a CSS mask so the icon takes the text colour. The browser/Mock build has no
// host, so `nativeSymbolsAvailable()` is false there and the UI draws Lucide instead.

/** What the host answers: a base64 PNG of the symbol, or a rejection for an unknown name. */
export interface SymbolBackend {
  png(name: string, pointSize: number, scale: number): Promise<string>;
}

const tauriBackend: SymbolBackend = {
  png: (name, pointSize, scale) => invoke<string>('symbol_png', { name, pointSize, scale }),
};

// `undefined`: the default (the host inside the app, none in the browser). `null`: none, whatever the environment.
let override: SymbolBackend | null | undefined;

/** Replace the backend (tests). `undefined` restores the default; `null` forces "no native symbols". */
export function setSymbolBackend(b: SymbolBackend | null | undefined): void {
  override = b;
  cache.clear();
}

const backend = (): SymbolBackend | null => (override !== undefined ? override : inTauri() ? tauriBackend : null);

export const nativeSymbolsAvailable = (): boolean => backend() !== null;

/** Device pixels per CSS pixel, as the host takes it (1 to 4). */
export const symbolScale = (): number => Math.min(4, Math.max(1, Math.ceil(globalThis.devicePixelRatio || 1)));

// Per name and size: the PNG, `null` once the host said no, or the request still running.
const cache = new Map<string, string | null | Promise<string | null>>();

const keyOf = (name: string, pointSize: number, scale: number) => `${name}@${pointSize}x${scale}`;

/** The answer if it is already known: the PNG, `null` (no such symbol), `undefined` (not asked yet or pending). */
export function peekSymbol(name: string, pointSize: number, scale = symbolScale()): string | null | undefined {
  const hit = cache.get(keyOf(name, pointSize, scale));
  return hit instanceof Promise ? undefined : hit;
}

/** Ask the host once per name and size. Resolves to `null` when there is no host or the symbol is unknown. */
export function loadSymbol(name: string, pointSize: number, scale = symbolScale()): Promise<string | null> {
  const b = backend();
  if (!b) return Promise.resolve(null);
  const key = keyOf(name, pointSize, scale);
  const hit = cache.get(key);
  if (hit !== undefined) return Promise.resolve(hit);
  const pending = b.png(name, pointSize, scale).then(
    (png) => png || null,
    () => null,
  );
  cache.set(key, pending);
  void pending.then((png) => {
    // a backend swap (tests) clears the cache: do not write into the new one
    if (cache.get(key) === pending) cache.set(key, png);
  });
  return pending;
}
