/** `localStorage['lotse-term-renderer'] = 'dom'` forces xterm's DOM renderer: a workaround if a GPU driver mangles WebGL, and what e2e tests use to read terminal text. */
export const RENDERER_KEY = 'lotse-term-renderer';

export function wantsWebgl(): boolean {
  try {
    return localStorage.getItem(RENDERER_KEY) !== 'dom';
  } catch {
    return true;
  }
}
