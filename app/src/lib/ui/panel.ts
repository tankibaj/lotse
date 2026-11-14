import { useSyncExternalStore } from 'react';
import type { SessionId } from '../client/types';

// The shared "what is open beside / over the terminal" state: at most one right panel (the mock's `panel`) and
// at most one full-window review (the mock's `#diff`). The shell writes it (toolbar, shortcuts, `togglePanel` /
// `openReview` commands); `panels/RightPanel` and `review/` read it.

export const PANEL_NAMES = ['session', 'changes', 'files', 'transcript', 'usage'] as const;
export type PanelName = (typeof PANEL_NAMES)[number];

export const isPanelName = (name: string | undefined): name is PanelName => PANEL_NAMES.some((p) => p === name);

export interface ReviewTarget {
  sessionId: SessionId;
  /** The file to scroll to; none: the first changed file. */
  file?: string;
}

let panel: PanelName | null = null;
let review: ReviewTarget | null = null;
const panelListeners = new Set<() => void>();
const reviewListeners = new Set<() => void>();

const notify = (listeners: Set<() => void>) => {
  for (const l of [...listeners]) l();
};

export const getOpenPanel = (): PanelName | null => panel;

/** Open `name` (closing any other panel), or close it when it is the open one. */
export function togglePanel(name: PanelName): void {
  panel = panel === name ? null : name;
  notify(panelListeners);
}

export function closePanel(): void {
  if (panel === null) return;
  panel = null;
  notify(panelListeners);
}

export function subscribePanel(listener: () => void): () => void {
  panelListeners.add(listener);
  return () => panelListeners.delete(listener);
}

/** React hook: re-renders when the open panel changes. */
export function useOpenPanel(): PanelName | null {
  return useSyncExternalStore(subscribePanel, getOpenPanel);
}

export const getReview = (): ReviewTarget | null => review;

export function openReview(sessionId: SessionId, file?: string): void {
  review = file === undefined ? { sessionId } : { sessionId, file };
  notify(reviewListeners);
}

export function closeReview(): void {
  if (review === null) return;
  review = null;
  notify(reviewListeners);
}

export function subscribeReview(listener: () => void): () => void {
  reviewListeners.add(listener);
  return () => reviewListeners.delete(listener);
}

/** React hook: re-renders when the review opens, changes target or closes. */
export function useReview(): ReviewTarget | null {
  return useSyncExternalStore(subscribeReview, getReview);
}
