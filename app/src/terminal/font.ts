export const DEFAULT_FONT_SIZE = 13;
export const MIN_FONT_SIZE = 9;
export const MAX_FONT_SIZE = 32;
/** xterm multiplies the glyph height; 1.3 gives the mock's 19.5 px row at 13 px on a 2x display (CSS `line-height: 1.5` is relative to the font size). */
export const LINE_HEIGHT = 1.3;
/** 500 where the family has a Medium face (SF Mono, JetBrains Mono): WKWebView draws text lighter than Chrome. Menlo has none and stays Regular. */
export const FONT_WEIGHT = '500';
export const FONT_WEIGHT_BOLD = '700';

/** Behind a user-chosen family. The mock's `.mono` stack starts with `ui-monospace`, which canvas fonts (WebGL renderer) do not resolve, so SF Mono and Menlo are named. */
const FALLBACK = '"SF Mono", Menlo, Monaco, monospace';

/** CSS `font-family` for the terminal: the chosen family first, then the mock's mono stack. Empty or "System" names use the stack alone. */
export function terminalFontFamily(name: string | undefined): string {
  const family = (name ?? '').trim().replace(/["\\]/g, '');
  if (family === '' || /^system\b/i.test(family)) return FALLBACK;
  return `"${family}", ${FALLBACK}`;
}

export function clampFontSize(size: number): number {
  if (!Number.isFinite(size)) return DEFAULT_FONT_SIZE;
  return Math.min(MAX_FONT_SIZE, Math.max(MIN_FONT_SIZE, Math.round(size)));
}
