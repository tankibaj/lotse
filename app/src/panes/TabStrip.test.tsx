import { act, fireEvent, render, screen, waitFor } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { ClientProvider, MockClient } from '../lib/client';
import { onUiCommand } from '../lib/ui/commands';
import { getSelectedSessionId, selectSession } from '../lib/ui/selection';
import { TabStrip } from './TabStrip';

let client: MockClient;
const mount = async () => {
  render(
    <ClientProvider client={client}>
      <TabStrip />
    </ClientProvider>,
  );
  await screen.findAllByRole('tab');
};

beforeEach(() => {
  client = new MockClient({ script: false });
  selectSession('a1');
});
afterEach(() => {
  client.close();
  selectSession(null);
});

describe('TabStrip', () => {
  it("shows one tab per session of the selected session's project, the selected one marked", async () => {
    await mount();
    const tabs = screen.getAllByRole('tab');
    expect(tabs.map((t) => t.textContent)).toEqual(['In progressFix invoice roundingbilling-api', 'IdleFlyway migration V42billing-api']);
    expect(tabs[0]).toHaveAttribute('aria-selected', 'true');
    expect(tabs[1]).toHaveAttribute('aria-selected', 'false');
    expect(screen.getByRole('tablist')).toHaveAccessibleName('Sessions in billing-api');
  });

  it('clicking a tab selects that session', async () => {
    await mount();
    fireEvent.click(screen.getAllByRole('tab')[1] as HTMLElement);
    expect(getSelectedSessionId()).toBe('a2');
    expect(screen.getAllByRole('tab')[1]).toHaveAttribute('aria-selected', 'true');
  });

  it('switching to another project swaps the tabs', async () => {
    await mount();
    act(() => selectSession('a3'));
    await waitFor(() => expect(screen.getByRole('tablist')).toHaveAccessibleName('Sessions in praxis-web'));
  });

  it('arrow keys move between tabs and wrap', async () => {
    await mount();
    fireEvent.keyDown(screen.getAllByRole('tab')[0] as HTMLElement, { key: 'ArrowRight' });
    expect(getSelectedSessionId()).toBe('a2');
    fireEvent.keyDown(screen.getAllByRole('tab')[1] as HTMLElement, { key: 'ArrowRight' });
    expect(getSelectedSessionId()).toBe('a1');
    fireEvent.keyDown(screen.getAllByRole('tab')[0] as HTMLElement, { key: 'ArrowLeft' });
    expect(getSelectedSessionId()).toBe('a2');
  });

  it('a status change shows up on the tab', async () => {
    await mount();
    act(() => client.setStatus('a2', 'fail'));
    await waitFor(() => expect(screen.getAllByRole('tab')[1]).toHaveTextContent('Failed'));
  });

  it('the + button asks for a new session', async () => {
    await mount();
    const handler = vi.fn();
    const off = onUiCommand('openNewSession', handler);
    fireEvent.click(screen.getByRole('button', { name: 'New session' }));
    expect(handler).toHaveBeenCalledTimes(1);
    off();
  });

  it('shows no tabs before a session is selected, but keeps the + button', () => {
    selectSession(null);
    render(
      <ClientProvider client={client}>
        <TabStrip />
      </ClientProvider>,
    );
    expect(screen.queryAllByRole('tab')).toHaveLength(0);
    expect(screen.getByRole('button', { name: 'New session' })).toBeInTheDocument();
  });
});
