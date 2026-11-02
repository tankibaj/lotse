import { act, fireEvent, screen, waitFor } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { describe, expect, it } from 'vitest';
import { MockClient } from '../lib/client';
import { getSelectedSessionId, selectSession } from '../lib/ui/selection';
import { mount } from '../sidebar/testUtils';
import { StatusBar } from './StatusBar';

async function ready() {
  const view = mount(<StatusBar />);
  await screen.findByText(/lotsed running/);
  return view;
}

const key = (code: string, init: KeyboardEventInit = {}) =>
  act(() => void fireEvent.keyDown(window, { code, key: code.startsWith('Key') ? code.slice(3).toLowerCase() : code, metaKey: true, ...init }));

describe('StatusBar', () => {
  it('shows need-you count, daemon state with session count, LAN Off and tokens as —', async () => {
    await ready();
    const bar = screen.getByRole('contentinfo');
    expect(bar).toHaveTextContent('▲ 2 need you');
    expect(bar).toHaveTextContent('● lotsed running · 7 sessions');
    expect(bar).toHaveTextContent('LAN Off');
    expect(bar).toHaveTextContent('Today — tokens · —');
  });

  it('shows the git line of the selected session: branch, worktree, ahead, dirty; "No git" for plain folders', async () => {
    await ready();
    const git = await screen.findByTestId('status-git');
    expect(git).toHaveTextContent('lotse/fix-rounding'); // the footer shows the full branch name, as the mock
    expect(git).toHaveTextContent('worktree');
    expect(git).toHaveTextContent('↑2');
    expect(git).toHaveTextContent('±3');
    act(() => selectSession('a7'));
    expect(screen.getByTestId('status-git')).toHaveTextContent('No git');
    act(() => selectSession('a5'));
    expect(screen.getByTestId('status-git')).not.toHaveTextContent('worktree'); // main checkout
  });

  it('follows status changes', async () => {
    const { client } = await ready();
    act(() => client.setStatus('a5', 'fail'));
    expect(screen.getByRole('button', { name: /3 need you/ })).toBeInTheDocument();
  });

  it('shows the daemon as not running while disconnected, and running again after reconnect', async () => {
    const { client } = await ready();
    act(() => client.setConnection('disconnected'));
    expect(screen.getByText(/lotsed not running/)).toBeInTheDocument();
    act(() => client.setConnection('reconnected'));
    expect(await screen.findByText(/lotsed running/)).toBeInTheDocument();
  });

  it('bell pauses and resumes notifications through the daemon settings', async () => {
    const { client } = await ready();
    const bell = screen.getByRole('button', { name: 'Pause notifications' });
    expect(bell).toHaveAttribute('aria-pressed', 'false');
    await userEvent.click(bell);
    await waitFor(() => expect(bell).toHaveAttribute('aria-pressed', 'true'));
    expect((await client.getSettings()).notifications.paused).toBe(true);
    expect(bell).toHaveTextContent('Paused');
    await userEvent.click(bell);
    await waitFor(() => expect(bell).toHaveAttribute('aria-pressed', 'false'));
  });
});

describe('session shortcuts (work with the sidebar hidden)', () => {
  it('⌘J jumps to the next session that needs you and wraps', async () => {
    await ready();
    await waitFor(() => expect(getSelectedSessionId()).toBe('a1'));
    key('KeyJ');
    expect(getSelectedSessionId()).toBe('a3');
    key('KeyJ');
    expect(getSelectedSessionId()).toBe('a4');
    key('KeyJ');
    expect(getSelectedSessionId()).toBe('a3');
  });

  it('⌘J follows the typed letter, so Dvorak/Colemak users keep their ⌘J (and their ⌘C)', async () => {
    await ready();
    await waitFor(() => expect(getSelectedSessionId()).toBe('a1'));
    key('KeyH', { key: 'j' }); // Dvorak: the key labelled J sits where QWERTY has H
    expect(getSelectedSessionId()).toBe('a3');
    key('KeyJ', { key: 'c' }); // Dvorak: the key at QWERTY's J types C: must not be hijacked
    expect(getSelectedSessionId()).toBe('a3');
  });

  it('⌘1…9 pick the nth session of the current project', async () => {
    await ready();
    await waitFor(() => expect(getSelectedSessionId()).toBe('a1'));
    key('Digit2');
    expect(getSelectedSessionId()).toBe('a2');
    key('Digit9'); // no 9th session: unchanged
    expect(getSelectedSessionId()).toBe('a2');
  });

  it('⌥⌘N toggles pause', async () => {
    const { client } = await ready();
    key('KeyN', { altKey: true });
    await waitFor(async () => expect((await client.getSettings()).notifications.paused).toBe(true));
  });

  it('ignores keys without ⌘', async () => {
    await ready();
    await waitFor(() => expect(getSelectedSessionId()).toBe('a1'));
    act(() => void fireEvent.keyDown(window, { code: 'KeyJ' }));
    expect(getSelectedSessionId()).toBe('a1');
  });
});

it('the MockClient script (a5 done, a2 fails) updates the need-you count live', async () => {
  const client = new MockClient({ script: true, tickMs: 0 });
  mount(<StatusBar />, client);
  await screen.findByText(/lotsed running/);
  act(() => client.tick(16_000)); // a2 run → fail
  expect(screen.getByRole('button', { name: /3 need you/ })).toBeInTheDocument();
  client.close();
});
