/**
 * Open a terminal link in the default browser. Only http(s) is allowed: terminal output is untrusted
 * text, so `file:`, `javascript:` and custom schemes never run.
 * Unverified: inside Tauri `window.open` may not reach the system browser; WP8 may route this through
 * the Tauri opener (needs a pinned dependency, see the report's Requests).
 */
export function openExternal(uri: string): boolean {
  let url: URL;
  try {
    url = new URL(uri);
  } catch {
    return false;
  }
  if (url.protocol !== 'http:' && url.protocol !== 'https:') return false;
  window.open(url.href, '_blank', 'noopener,noreferrer');
  return true;
}

/** iTerm2 behaviour: a link opens on ⌘-click (⌃-click off macOS), a plain click only selects. */
export const isLinkClick = (e: Pick<MouseEvent, 'metaKey' | 'ctrlKey'>): boolean => e.metaKey || e.ctrlKey;
