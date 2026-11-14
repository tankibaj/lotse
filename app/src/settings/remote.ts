/** What the status bar and Settings › Remote both say about LAN access. */
export interface LanState {
  on: boolean;
  /** Status bar text. */
  label: string;
  /** Status bar tooltip. */
  tooltip: string;
}

/**
 * LAN access. Alpha has none: the settings model (`Settings`) has no remote field and the daemon opens no
 * network listener, so this is always off. Settings › Remote and the status bar both read it here, so
 * they cannot disagree. When a LAN setting arrives (Beta), derive `on` from it in this one place.
 */
export function lanState(): LanState {
  const on: boolean = false;
  return on
    ? { on, label: 'LAN On', tooltip: 'LAN access is on (Settings › Remote)' }
    : { on, label: 'LAN Off', tooltip: 'LAN access is off (Settings › Remote)' };
}
