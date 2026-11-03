import { useSyncExternalStore } from 'react';
import type { AgentKind, ProjectId, SessionId } from '../lib/client/types';

// The one open overlay (the mock's `closeAll()` then show): opening any of them closes the others.
// Sheets, Settings and Palette read it; their UI-command and key bindings write it.

export type SettingsSection = 'Agents' | 'Notifications' | 'Appearance' | 'Remote' | 'Privacy' | 'Shortcuts';

/** What the New session sheet had when the user left it for Settings (its "Change…" link), so it can come back. */
export interface NewSessionDraft {
  agent: AgentKind;
  worktree: boolean;
  branch: string;
  prompt: string;
}

export type Overlay =
  | { name: 'newSession'; projectId?: ProjectId; draft?: NewSessionDraft }
  | { name: 'addProject'; repos?: { root: string; repos: string[] } }
  | { name: 'shortcuts' }
  | { name: 'palette' }
  | { name: 'settings'; section: SettingsSection; returnTo?: Overlay }
  | { name: 'projectStyle'; projectId: ProjectId; x: number; y: number }
  | { name: 'changes'; sessionId: SessionId }
  | { name: 'transcript'; sessionId: SessionId };

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

/**
 * Open the Settings page, optionally on a section (the mock's `openSet(k)`). `returnTo`: the overlay that
 * opens again when Settings is left with Back or Esc (`closeSettings`), e.g. the New session sheet.
 */
export const openSettings = (section: SettingsSection = 'Agents', returnTo?: Overlay) =>
  openOverlay(returnTo ? { name: 'settings', section, returnTo } : { name: 'settings', section });

/** Leave Settings: back to the overlay it was opened from, else to the terminal. */
export function closeSettings(): void {
  if (current?.name === 'settings' && current.returnTo) openOverlay(current.returnTo);
  else closeOverlay();
}

/**
 * Open the project colour and icon popover, anchored below `anchor` (a DOM rect, e.g. the project's
 * tile). The sidebar's project menu calls this ("Icon & colour…").
 */
export function openProjectStylePicker(projectId: ProjectId, anchor?: { left: number; bottom: number }): void {
  openOverlay({ name: 'projectStyle', projectId, x: anchor?.left ?? 24, y: (anchor?.bottom ?? 80) + 6 });
}
