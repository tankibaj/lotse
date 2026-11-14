import { render } from '@testing-library/react';
import { afterEach } from 'vitest';
import { ClientProvider, MockClient } from '../lib/client';
import { Palette } from '../palette';
import { SettingsPage } from '../settings';
import { selectSession } from '../lib/ui/selection';
import { setHostBridge } from './hostBridge';
import { closeOverlay } from './overlay';
import { Sheets } from './index';

/** Tests only: WP5's three slots over a quiet MockClient (no scripted status changes). */
export function mountWp5() {
  const client = new MockClient({ script: false });
  const view = render(
    <ClientProvider client={client}>
      <Sheets />
      <SettingsPage />
      <Palette />
    </ClientProvider>,
  );
  return { client, ...view };
}

afterEach(() => {
  closeOverlay();
  selectSession(null);
  setHostBridge(null);
});
