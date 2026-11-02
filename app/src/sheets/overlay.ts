import { useSyncExternalStore } from 'react';
import type { ProjectId } from '../lib/client/types';

// The one open overlay (the mock's `closeAll()` then show): opening any of them closes the others.
// Sheets, Settings and Palette read it; their UI-command and key bindings write it.

export type SettingsSection = 'Agents' | 'Notifications' | 'Appearance' | 'Remote' | 'Privacy' | 'Shortcuts';

export type Overlay =
  | { name: 'newSession'; projectId?: ProjectId }
  | { name: 'addProject'; repos?: { root: string; repos: string[] } }
  | { name: 'shortcuts' }
  | { name: 'palette' }
  | { name: 'settings'; section: SettingsSection }
  | { name: 'projectStyle'; projectId: ProjectId; x: number; y: number };

let current: Overlay | null = null;
const listeners = new Set<() => void>();

export const getOverlay = (): Overlay | null => current;

export function openOverlay(next: Overlay): void {
  current = next;
  for (const l of [...listeners]) l();
}

export function closeOverlay(): void {
  if (current === null) return;
  current = null;
  for (const l of [...listeners]) l();
}

export function toggleOverlay(next: Overlay): void {
  if (current?.name === next.name) closeOverlay();
  else openOverlay(next);
}

const subscribe = (l: () => void) => {
  listeners.add(l);
  return () => listeners.delete(l);
};

export function useOverlay(): Overlay | null {
  return useSyncExternalStore(subscribe, getOverlay);
}

/** Open the Settings page, optionally on a section (the mock's `openSet(k)`). */
export const openSettings = (section: SettingsSection = 'Agents') => openOverlay({ name: 'settings', section });

/**
 * Open the project colour and icon popover, anchored below `anchor` (a DOM rect, e.g. the project's
 * tile). The sidebar's project menu calls this ("Icon & colour…").
 */
export function openProjectStylePicker(projectId: ProjectId, anchor?: { left: number; bottom: number }): void {
  openOverlay({ name: 'projectStyle', projectId, x: anchor?.left ?? 24, y: (anchor?.bottom ?? 80) + 6 });
}
