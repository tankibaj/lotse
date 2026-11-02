import type { AppearanceSettings } from '../lib/client/types';

export const INTERFACE_FONTS = ['System (SF Pro)', 'Helvetica Neue', 'Inter'];
export const TERMINAL_FONTS = ['SF Mono', 'Menlo', 'JetBrains Mono'];
export const INTERFACE_SIZE = { min: 11, max: 17 };
export const TERMINAL_SIZE = { min: 10, max: 24 };

const SYSTEM = 'System (SF Pro)';
const clamp = (n: number, r: { min: number; max: number }) => Math.min(r.max, Math.max(r.min, Math.round(n) || r.min));

/** CSS family list for a stored font name. `System (SF Pro)` means the app default (no override). */
export const interfaceStack = (name: string): string | null =>
  name === SYSTEM ? null : `'${name.replace(/'/g, '')}', -apple-system, BlinkMacSystemFont, system-ui, sans-serif`;
export const terminalStack = (name: string): string =>
  `'${name.replace(/'/g, '')}', ui-monospace, 'SF Mono', Menlo, monospace`;

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
