// Tiny UI command bus. Lets the shell (and any WP) ask another WP's overlay to open without importing it,
// so UI WPs never edit `shell/`. The owner of each overlay subscribes with `onUiCommand`.
//
//   openSettings, openPalette, openAddProject    -> WP5 (sheets, settings, palette)
//   openNewSession (optional project id)         -> WP5: the sheet preselects that project
//   openProjectStyle (project id)                -> WP5: colour and icon popover next to the project row
//   splitSession (session id)                    -> WP4: open the session in the right pane
//   splitSessionDown (session id)                -> WP4: open the session in a pane below the focused one
//   showChanges (session id)                     -> WP5: read-only `git diff` sheet of the session's folder
//   showTranscript (session id)                  -> WP5: read-only transcript sheet
//   toggleSidebar                                -> shell
//   togglePanel (panel name)                     -> shell: one right panel at a time (`lib/ui/panel`)
//   openReview (session id, optional file)       -> shell: the full-window review (`review/`)
//   openFile (session id, path)                  -> editor pane (no listener until it lands)
//   openComposer                                 -> composer: the prompt box over the focused pane

export type UiCommand =
  | 'showChanges'
  | 'showTranscript'
  | 'openSettings'
  | 'openPalette'
  | 'openNewSession'
  | 'openAddProject'
  | 'toggleSidebar'
  | 'openProjectStyle'
  | 'splitSession'
  | 'splitSessionDown'
  | 'togglePanel'
  | 'openReview'
  | 'openFile'
  | 'openComposer';

/** Optional payload: a project id, a session id or a panel name, depending on the command; `extra` is a file path. */
export type UiCommandHandler = (arg?: string, extra?: string) => void;

const handlers = new Map<UiCommand, Set<UiCommandHandler>>();

/** Subscribe to a command; returns the unsubscribe function. */
export function onUiCommand(command: UiCommand, handler: UiCommandHandler): () => void {
  const set = handlers.get(command) ?? new Set();
  set.add(handler);
  handlers.set(command, set);
  return () => set.delete(handler);
}

/** Fire a command. Returns the number of handlers that ran (0: nobody listens yet). */
export function emitUiCommand(command: UiCommand, arg?: string, extra?: string): number {
  const set = handlers.get(command);
  set?.forEach((h) => (extra === undefined ? h(arg) : h(arg, extra)));
  return set?.size ?? 0;
}
