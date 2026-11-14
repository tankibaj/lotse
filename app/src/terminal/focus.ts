// xterm.js takes Tab and Shift+Tab for the shell, which would trap keyboard focus inside a terminal
// (WCAG 2.1.2). Ctrl+Tab and Ctrl+Shift+Tab leave it: focus goes to the next or previous control, in page order.

const TABBABLE = 'a[href], button:not([disabled]), input:not([disabled]), select:not([disabled]), textarea:not([disabled]), [tabindex]:not([tabindex="-1"])';

const isReachable = (el: HTMLElement) => !el.closest('[hidden], [inert], [aria-hidden="true"]');

/**
 * Focus the control after (`delta` 1) or before (-1) `from` in document order, wrapping at the ends.
 * Terminals are skipped (⌘[ and ⌘] move between those). Returns whether something took focus.
 */
export function moveFocusOut(from: HTMLElement, delta: 1 | -1): boolean {
  const candidates = [...document.querySelectorAll<HTMLElement>(TABBABLE)].filter((el) => el !== from && el.tabIndex >= 0 && !el.closest('.xterm') && isReachable(el));
  if (candidates.length === 0) return false;
  const after = candidates.filter((el) => from.compareDocumentPosition(el) & Node.DOCUMENT_POSITION_FOLLOWING);
  const before = candidates.filter((el) => from.compareDocumentPosition(el) & Node.DOCUMENT_POSITION_PRECEDING);
  const target = delta === 1 ? (after[0] ?? candidates[0]) : (before[before.length - 1] ?? candidates[candidates.length - 1]);
  target?.focus();
  return target !== undefined && document.activeElement === target;
}
