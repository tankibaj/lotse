import { inTauri } from '../../sheets/hostBridge';
import type { SessionId } from '../../lib/client/types';

// Connectors › Manage: open an agent config file in the default text editor. Native only
// (`app/src-tauri/src/config_open.rs`, `open -t`, and only for a path the daemon listed for the session);
// in the browser there is nothing to open, so Manage is disabled.

export type ConfigOpener = (sessionId: SessionId, path: string) => Promise<void>;

const tauriOpener: ConfigOpener = async (sessionId, path) => {
  const { invoke } = await import('@tauri-apps/api/core');
  await invoke('open_agent_config', { sessionId, path });
};

let override: ConfigOpener | null | undefined;

/** Tests inject an opener (`null`: none, as in the browser); `undefined` restores the default. */
export function setConfigOpener(o: ConfigOpener | null | undefined): void {
  override = o;
}

export const configOpener = (): ConfigOpener | null => (override !== undefined ? override : inTauri() ? tauriOpener : null);
