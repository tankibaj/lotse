// Tiny UI command bus. Lets the shell (and any WP) ask another WP's overlay to open without importing it,
// so UI WPs never edit `shell/`. The owner of each overlay subscribes with `onUiCommand`.
//
//   openSettings, openPalette, openAddProject    -> WP5 (sheets, settings, palette)
//   openNewSession (optional project id)         -> WP5: the sheet preselects that project
//   openProjectStyle (project id)                -> WP5: colour and icon popover next to the project row
//   splitSession (session id)                    -> WP4: open the session in the right pane
//   splitSessionDown (session id)                -> WP4: open the session in a pane below the focused one
//   toggleSidebar                                -> shell

export type UiCommand =
  | 'openSettings'
  | 'openPalette'
  | 'openNewSession'
  | 'openAddProject'
  | 'toggleSidebar'
  | 'openProjectStyle'
  | 'splitSession'
  | 'splitSessionDown';

/** Optional payload: a project id or a session id, depending on the command. */
export type UiCommandHandler = (arg?: string) => void;

const handlers = new Map<UiCommand, Set<UiCommandHandler>>();

/** Subscribe to a command; returns the unsubscribe function. */
export function onUiCommand(command: UiCommand, handler: UiCommandHandler): () => void {
  const set = handlers.get(command) ?? new Set();
  set.add(handler);
  handlers.set(command, set);
  return () => set.delete(handler);
}

/** Fire a command. Returns the number of handlers that ran (0: nobody listens yet). */
export function emitUiCommand(command: UiCommand, arg?: string): number {
  const set = handlers.get(command);
  set?.forEach((h) => h(arg));
  return set?.size ?? 0;
}
