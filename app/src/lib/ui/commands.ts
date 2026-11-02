// Tiny UI command bus. Lets the shell (and any WP) ask another WP's overlay to open without importing it,
// so UI WPs never edit `shell/`. The owner of each overlay subscribes with `onUiCommand`.
//
//   openSettings, openPalette, openNewSession, openAddProject  -> WP5 (sheets, settings, palette)
//   toggleSidebar                                               -> shell

export type UiCommand = 'openSettings' | 'openPalette' | 'openNewSession' | 'openAddProject' | 'toggleSidebar';

const handlers = new Map<UiCommand, Set<() => void>>();

/** Subscribe to a command; returns the unsubscribe function. */
export function onUiCommand(command: UiCommand, handler: () => void): () => void {
  const set = handlers.get(command) ?? new Set();
  set.add(handler);
  handlers.set(command, set);
  return () => set.delete(handler);
}

/** Fire a command. Returns the number of handlers that ran (0: nobody listens yet). */
export function emitUiCommand(command: UiCommand): number {
  const set = handlers.get(command);
  set?.forEach((h) => h());
  return set?.size ?? 0;
}
