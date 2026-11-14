import type { DaemonClient } from './DaemonClient';
import { MockClient } from './MockClient';
import { RealClient, tauriTransport } from './realClient';

export type { ConnectionState, DaemonClient, Unsubscribe } from './DaemonClient';
export { MockClient, DEFAULT_SCRIPT, FIXTURE_NOW } from './MockClient';
export type { MockClientOptions, ScriptStep } from './MockClient';
export { RealClient } from './realClient';
export type { RealClientOptions, Transport } from './realClient';
export { ClientProvider, useClient } from './context';
export * from './types';

/** `VITE_LOTSE_CLIENT=mock|real` forces one; unset picks by environment. */
export type ClientChoice = 'mock' | 'real';

export function chooseClient(flag: string | undefined, inTauri: boolean): ClientChoice {
  if (flag === 'mock' || flag === 'real') return flag;
  return inTauri ? 'real' : 'mock';
}

/**
 * The client the app runs with. Inside the Tauri app: the real client over the daemon bridge.
 * In a plain browser (`npm run dev`, Playwright on the web build): `MockClient`, seeded with the mock's
 * data. `VITE_LOTSE_CLIENT=mock npm run tauri dev` keeps the mock inside the app window for UI work.
 */
export function createClient(): DaemonClient {
  const inTauri = typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window;
  const choice = chooseClient(import.meta.env.VITE_LOTSE_CLIENT as string | undefined, inTauri);
  if (choice === 'mock') return new MockClient();
  return new RealClient({
    onReady: (client) => {
      // Dock badge and focus reporting; loaded lazily so the client never imports `lib/state` statically.
      void import('./realNative').then(({ wireNativeShell }) => wireNativeShell(client, tauriTransport));
    },
  });
}
