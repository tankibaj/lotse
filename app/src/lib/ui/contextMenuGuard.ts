/** Text fields and the terminal keep the native menu (copy, paste, select all); xterm needs its own selection. */
const keepsNativeMenu = (target: EventTarget | null): boolean =>
  target instanceof Element &&
  (target.closest('.xterm') !== null ||
    target.closest('input:not([type=button],[type=checkbox],[type=radio],[type=range],[type=submit],[type=reset],[type=file],[type=image],[type=color])') !== null ||
    target.closest('textarea') !== null ||
    (target instanceof HTMLElement && target.isContentEditable));

/**
 * No WebKit default menu ("Reload", "Inspect"…) anywhere in the app: the sidebar and tabs bring their own.
 * Returns the function that removes the guard.
 */
export function installContextMenuGuard(target: Document = document): () => void {
  const onContextMenu = (e: MouseEvent) => {
    if (!keepsNativeMenu(e.target)) e.preventDefault();
  };
  target.addEventListener('contextmenu', onContextMenu);
  return () => target.removeEventListener('contextmenu', onContextMenu);
}
