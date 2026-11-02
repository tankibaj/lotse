import { act, fireEvent, render, screen } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { ClientProvider, MockClient } from '../lib/client';
import { closeOverlay, getOverlay } from '../sheets/overlay';
import { SETTINGS_MENU_EVENT, onMenuSettings } from './appMenu';
import { setIntegrationBridge } from './integration';
import { SettingsPage } from './SettingsPage';

// Only the Settings slot is mounted: the rest of the shell would talk to a real Tauri host.
const mountSettings = () =>
  render(
    <ClientProvider client={new MockClient({ script: false })}>
      <SettingsPage />
    </ClientProvider>,
  );

// the app menu's "Settings…" arrives as a Tauri event; capture the handler `listen` is given
const listeners = new Map<string, () => void>();
const unlisten = vi.fn();
vi.mock('@tauri-apps/api/event', () => ({
  listen: vi.fn(async (name: string, cb: () => void) => {
    listeners.set(name, cb);
    return unlisten;
  }),
}));

const tauri = (on: boolean) => {
  const w = window as unknown as Record<string, unknown>;
  if (on) w.__TAURI_INTERNALS__ = {};
  else delete w.__TAURI_INTERNALS__;
};
const flush = () => act(async () => {});

beforeEach(() => {
  listeners.clear();
  unlisten.mockClear();
  setIntegrationBridge(null);
});
afterEach(() => {
  closeOverlay();
  tauri(false);
  setIntegrationBridge(undefined);
});

describe('app menu › Settings…', () => {
  it('the menu event opens Settings', async () => {
    tauri(true);
    mountSettings();
    await flush();
    const fire = listeners.get(SETTINGS_MENU_EVENT);
    expect(fire).toBeDefined();
    expect(getOverlay()).toBeNull();
    act(() => fire?.());
    expect(await screen.findByRole('dialog', { name: 'Settings' })).toBeInTheDocument();
    expect(getOverlay()).toMatchObject({ name: 'settings' });
  });

  it('in the app ⌘, belongs to the menu accelerator: the webview key handler stays off', async () => {
    tauri(true);
    mountSettings();
    await flush();
    fireEvent.keyDown(window, { code: 'Comma', metaKey: true });
    expect(getOverlay()).toBeNull();
  });

  it('in the browser there is no menu: nothing is listened to and ⌘, opens Settings', async () => {
    mountSettings();
    await flush();
    expect(listeners.size).toBe(0);
    fireEvent.keyDown(window, { code: 'Comma', metaKey: true });
    expect(await screen.findByRole('dialog', { name: 'Settings' })).toBeInTheDocument();
  });

  it('stops listening when the page unmounts, also when that happens before listen answered', async () => {
    tauri(true);
    const view = mountSettings();
    await flush();
    view.unmount();
    await flush();
    expect(unlisten).toHaveBeenCalledTimes(1);

    const off = onMenuSettings(() => {});
    off(); // before the pending `listen` promise resolves
    await flush();
    expect(unlisten).toHaveBeenCalledTimes(2);
  });
});
