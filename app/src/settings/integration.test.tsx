import { act, screen, waitFor } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { afterEach, describe, expect, it, vi } from 'vitest';
import { MockClient } from '../lib/client';
import { emitUiCommand } from '../lib/ui/commands';
import { mountWp5 } from '../sheets/harness';
import { THEME_KEY, getThemePref } from '../theme';
import { parseStatus, setIntegrationBridge } from './integration';
import type { IntegrationBridge, IntegrationState } from './integration';

afterEach(() => {
  setIntegrationBridge(undefined);
  localStorage.clear();
  document.documentElement.classList.remove('dark');
});

const open = async () => {
  act(() => void emitUiCommand('openSettings'));
  return screen.findByRole('dialog', { name: 'Settings' });
};

function bridge(initial: IntegrationState) {
  const box = { state: initial };
  return {
    status: vi.fn<IntegrationBridge['status']>(async () => box.state),
    install: vi.fn<IntegrationBridge['install']>(async () => void (box.state = 'installed')),
    uninstall: vi.fn<IntegrationBridge['uninstall']>(async () => void (box.state = 'missing')),
  };
}

describe('lotse-hook status text', () => {
  it('maps the installer output to a state', () => {
    expect(parseStatus('installed\n')).toBe('installed');
    expect(parseStatus('not installed')).toBe('missing');
    expect(parseStatus('partial; missing: Stop')).toBe('outdated');
    expect(parseStatus('outdated; missing: current plugin version')).toBe('outdated');
  });
});

describe('Settings › Agents integrations (app only)', () => {
  it('installs and removes the Claude hooks and the OpenCode plugin', async () => {
    const b = bridge('missing');
    setIntegrationBridge(b);
    mountWp5();
    await open();
    const claude = await screen.findByTestId('integration-claude');
    expect(claude).toHaveTextContent('hooks not installed');
    await userEvent.click(screen.getByRole('button', { name: 'Install hooks' }));
    await waitFor(() => expect(screen.getByTestId('integration-claude')).toHaveTextContent('hooks installed'));
    expect(b.install).toHaveBeenCalledWith('claude');
    await userEvent.click(screen.getByRole('button', { name: 'Remove hooks' }));
    await waitFor(() => expect(screen.getByTestId('integration-claude')).toHaveTextContent('hooks not installed'));
    expect(b.uninstall).toHaveBeenCalledWith('claude');
    // OpenCode has its own row and noun
    expect(screen.getByTestId('integration-opencode')).toHaveTextContent('plugin not installed');
    await userEvent.click(screen.getByRole('button', { name: 'Install plugin' }));
    expect(b.install).toHaveBeenCalledWith('opencode');
  });

  it('shows the installer\'s refusal (a foreign plugin file is never overwritten)', async () => {
    const b = bridge('missing');
    b.install.mockRejectedValue('lotse-status.js exists and is not the Lotse plugin; not overwriting it');
    setIntegrationBridge(b);
    mountWp5();
    await open();
    await userEvent.click(await screen.findByRole('button', { name: 'Install plugin' }));
    expect(await screen.findByRole('alert')).toHaveTextContent('not overwriting it');
  });

  it('shows nothing in the browser (no bridge)', async () => {
    setIntegrationBridge(null);
    mountWp5();
    await open();
    await screen.findByLabelText('Claude Code launch command');
    expect(screen.queryByTestId('integration-claude')).toBeNull();
    expect(screen.queryByRole('button', { name: /Install/ })).toBeNull();
  });
});

describe('theme: the daemon wins once connected', () => {
  it('adopts appearance.theme from the daemon over the local pref, and follows later changes', async () => {
    // a stale local pref that differs from what the daemon has saved
    const probe = new MockClient({ script: false });
    const saved = (await probe.getSettings()).appearance.theme;
    localStorage.setItem(THEME_KEY, saved === 'dark' ? 'light' : 'dark');
    const { client } = mountWp5();
    await waitFor(() => expect(getThemePref()).toBe(saved));

    const set = async (theme: 'light' | 'dark') => {
      const now = await client.getSettings();
      await act(() => client.updateSettings({ ...now, appearance: { ...now.appearance, theme } }).then(() => undefined));
    };
    await set('dark');
    await waitFor(() => expect(getThemePref()).toBe('dark'));
    expect(document.documentElement.classList.contains('dark')).toBe(true);
    await set('light');
    await waitFor(() => expect(getThemePref()).toBe('light'));
    expect(document.documentElement.classList.contains('dark')).toBe(false);
  });
});
