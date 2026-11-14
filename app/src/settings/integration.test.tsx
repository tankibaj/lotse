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
  it('offers no install action and says status is automatic', async () => {
    setIntegrationBridge(bridge('missing'));
    mountWp5();
    await open();
    expect(await screen.findByTestId('live-status-claude')).toHaveTextContent(
      'Live status: automatic in Lotse panes (nothing is written to your Claude settings)',
    );
    expect(screen.getByTestId('live-status-opencode')).toHaveTextContent('nothing is written to your OpenCode config');
    expect(screen.queryByRole('button', { name: /install|plugin/i })).toBeNull();
    expect(screen.queryByRole('button', { name: /remove old global hooks/i })).toBeNull(); // nothing installed: no cleanup
  });

  it('shows cleanup only while an old global install is present, and removes it', async () => {
    const b = bridge('installed');
    setIntegrationBridge(b);
    mountWp5();
    await open();
    const remove = await screen.findByRole('button', { name: 'Remove old global hooks for Claude Code' });
    expect(screen.getAllByRole('button', { name: /remove old global hooks/i })).toHaveLength(2); // Claude Code + OpenCode
    expect(screen.queryByRole('button', { name: /install/i })).toBeNull();
    await userEvent.click(remove);
    await waitFor(() => expect(b.uninstall).toHaveBeenCalledWith('claude'));
    await waitFor(() => expect(screen.queryByRole('button', { name: 'Remove old global hooks for Claude Code' })).toBeNull());
    expect(screen.getByRole('button', { name: 'Remove old global hooks for OpenCode' })).toBeInTheDocument(); // other agent untouched
  });

  it('also offers cleanup for a partial or outdated old install', async () => {
    setIntegrationBridge(bridge('outdated'));
    mountWp5();
    await open();
    expect(await screen.findByRole('button', { name: 'Remove old global hooks for Claude Code' })).toBeEnabled();
  });

  it('shows the uninstaller\'s error', async () => {
    const b = bridge('installed');
    b.uninstall.mockRejectedValue('settings.json is not valid JSON');
    setIntegrationBridge(b);
    mountWp5();
    await open();
    await userEvent.click(await screen.findByRole('button', { name: 'Remove old global hooks for Claude Code' }));
    expect(await screen.findAllByRole('alert')).not.toHaveLength(0);
    expect(screen.getAllByRole('alert')[0]).toHaveTextContent('not valid JSON');
  });

  it('shows no cleanup in the browser (no bridge), only the live-status line', async () => {
    setIntegrationBridge(null);
    mountWp5();
    await open();
    await screen.findByLabelText('Claude Code launch command');
    expect(screen.getByTestId('live-status-claude')).toBeInTheDocument();
    expect(screen.queryByRole('button', { name: /install|remove old/i })).toBeNull();
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
