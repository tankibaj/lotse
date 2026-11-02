import type { DaemonClient } from './DaemonClient';
import { MockClient } from './MockClient';

export type { ConnectionState, DaemonClient, Unsubscribe } from './DaemonClient';
export { MockClient, DEFAULT_SCRIPT, FIXTURE_NOW } from './MockClient';
export type { MockClientOptions, ScriptStep } from './MockClient';
export { ClientProvider, useClient } from './context';
export * from './types';

/**
 * The client the app runs with. WP0: always `MockClient`, seeded with the mock's data (`npm run dev`).
 * WP8 switches to the real client inside Tauri and keeps `MockClient` for the browser.
 */
export function createClient(): DaemonClient {
  return new MockClient();
}
