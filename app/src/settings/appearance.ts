import type { AppearanceSettings } from '../lib/client/types';
import { terminalFontFamily } from '../terminal/font';

export const INTERFACE_SIZE = { min: 11, max: 17 };
export const TERMINAL_SIZE = { min: 10, max: 24 };

/** The stored interface font that means "no override": the system UI font. */
export const SYSTEM_FONT = 'System (SF Pro)';
const clamp = (n: number, r: { min: number; max: number }) => Math.min(r.max, Math.max(r.min, Math.round(n) || r.min));

/** CSS family list for a stored font name. `System (SF Pro)` means the app default (no override). */
export const interfaceStack = (name: string): string | null =>
  name === SYSTEM_FONT ? null : `'${name.replace(/['\\]/g, '')}', -apple-system, BlinkMacSystemFont, system-ui, sans-serif`;
/** The chosen family, then SF Mono, then Menlo (the terminal's own stack), so missing glyphs still render. */
export const terminalStack = terminalFontFamily;
/** A family name as one quoted CSS value, for rendering an <option> in its own font. */
export const cssFamily = (name: string): string => `'${name.replace(/['\\]/g, '')}'`;

/**
 * Apply fonts to the document. Interface font/size go on <body>. The terminal font is published as CSS
 * variables (`--lotse-terminal-font`, `--lotse-terminal-font-size`) for the terminal view to read.
 * Theme is not applied here: `theme/` owns it (local pref, like the mock).
 */
export function applyAppearance(a: AppearanceSettings): void {
  const body = document.body.style;
  const stack = interfaceStack(a.interface_font);
  if (stack) body.fontFamily = stack;
  else body.removeProperty('font-family');
  body.fontSize = `${clamp(a.interface_font_size, INTERFACE_SIZE)}px`;
  const root = document.documentElement.style;
  root.setProperty('--lotse-terminal-font', terminalStack(a.terminal_font));
  root.setProperty('--lotse-terminal-font-size', `${clamp(a.terminal_font_size, TERMINAL_SIZE)}px`);
}
