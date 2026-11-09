import { act, render, screen, waitFor } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it } from 'vitest';
import { ClientProvider, MockClient } from '../lib/client';
import { selectSession } from '../lib/ui/selection';
import { PaneBar } from './PaneBar';
import { TabStrip } from './TabStrip';
import { useSessions } from './useSessions';

// Title sync: the title the agent sets (mod: session title, or the first prompt's summary) reaches the tab and the pane bar live.

let client: MockClient;
beforeEach(() => {
  client = new MockClient({ script: false });
  selectSession('a1');
});
afterEach(() => {
  client.close();
  selectSession(null);
});

function Bar() {
  const { sessions, projects } = useSessions();
  const s = sessions.find((x) => x.id === 'a1');
  return s ? <PaneBar session={s} project={projects.find((p) => p.id === s.project_id)} focused /> : null;
}

describe('title sync', () => {
  it('a new session title shows on the tab and the pane bar without a reload', async () => {
    render(
      <ClientProvider client={client}>
        <TabStrip />
        <Bar />
      </ClientProvider>,
    );
    await screen.findByRole('tab', { name: /Fix invoice rounding/ });
    act(() => void client.renameSession('a1', 'Round half to even'));
    await waitFor(() => expect(screen.getByRole('tab', { name: /Round half to even/ })).toBeInTheDocument());
    expect(screen.getByTestId('pane-status')).toHaveTextContent('Round half to even');
    expect(screen.queryByRole('tab', { name: /Fix invoice rounding/ })).toBeNull();
  });

  it('shows the window title of the selected session: "<title> · Lotse"', async () => {
    render(
      <ClientProvider client={client}>
        <TabStrip />
      </ClientProvider>,
    );
    await waitFor(() => expect(document.title).toBe('Fix invoice rounding · Lotse'));
    act(() => void client.renameSession('a1', 'Round half to even'));
    await waitFor(() => expect(document.title).toBe('Round half to even · Lotse'));
  });
});
