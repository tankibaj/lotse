/**
 * Open a terminal link in the default browser. Only http(s) is allowed: terminal output is untrusted
 * text, so `file:`, `javascript:` and custom schemes never run. Inside the app the link goes to the
 * host (`open_url`, which checks the scheme again and runs `/usr/bin/open`); a webview `window.open`
 * would not leave the app. In the browser (dev server) `window.open` is used.
 */
export function openExternal(uri: string): boolean {
  let url: URL;
  try {
    url = new URL(uri);
  } catch {
    return false;
  }
  if (url.protocol !== 'http:' && url.protocol !== 'https:') return false;
  if (typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window) {
    void import('@tauri-apps/api/core').then(({ invoke }) => invoke('open_url', { url: url.href })).catch(() => {});
    return true;
  }
  window.open(url.href, '_blank', 'noopener,noreferrer');
  return true;
}

/** iTerm2 behaviour: a link opens on ⌘-click (⌃-click off macOS), a plain click only selects. */
export const isLinkClick = (e: Pick<MouseEvent, 'metaKey' | 'ctrlKey'>): boolean => e.metaKey || e.ctrlKey;
