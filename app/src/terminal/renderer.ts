/**
 * `localStorage['lotse-term-renderer'] = 'webgl'` opts in to xterm's WebGL renderer. The default is the DOM renderer:
 * it draws text with the browser's own text engine, so glyphs have the weight, smoothing and fractional cell width of the
 * mock's `.term` (and of native macOS text in WKWebView). The WebGL glyph atlas is thinner and rounds the cell width down
 * to whole device pixels. Use WebGL only for very heavy output.
 */
export const RENDERER_KEY = 'lotse-term-renderer';

export function wantsWebgl(): boolean {
  try {
    return localStorage.getItem(RENDERER_KEY) === 'webgl';
  } catch {
    return false;
  }
}
