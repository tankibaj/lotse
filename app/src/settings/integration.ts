import { inTauri } from '../sheets/hostBridge';

// Settings › Agents: cleanup of the old global status hooks of an agent. Lotse never installs them
// any more (hooks are injected per pane). Native only (the app runs `lotse-hook integration <agent>
// uninstall|status`, `app/src-tauri/src/host.rs`); in the browser there is nothing to remove, so the
// controls do not appear.

export type IntegrationId = 'claude' | 'opencode';
export type IntegrationState = 'installed' | 'missing' | 'outdated';

export interface IntegrationBridge {
  status(id: IntegrationId): Promise<IntegrationState>;
  uninstall(id: IntegrationId): Promise<void>;
}

/** `lotse-hook integration … status` prints `installed`, `not installed`, or `partial…` / `outdated…`. */
export const parseStatus = (text: string): IntegrationState =>
  text.trim() === 'installed' ? 'installed' : text.trim().startsWith('not installed') ? 'missing' : 'outdated';

const tauriBridge: IntegrationBridge = {
  async status(id) {
    const { invoke } = await import('@tauri-apps/api/core');
    return parseStatus(await invoke<string>('integration_status', { agent: id }));
  },
  async uninstall(id) {
    const { invoke } = await import('@tauri-apps/api/core');
    await invoke('integration_uninstall', { agent: id });
  },
};

let override: IntegrationBridge | null | undefined;

/** Tests inject a bridge (`null`: none, as in the browser); `undefined` restores the default. */
export function setIntegrationBridge(b: IntegrationBridge | null | undefined): void {
  override = b;
}

export const integrationBridge = (): IntegrationBridge | null => (override !== undefined ? override : inTauri() ? tauriBridge : null);
