import { act, fireEvent, screen } from '@testing-library/react';
import { describe, expect, it } from 'vitest';
import { MockClient } from '../lib/client';
import { mount } from '../sidebar/testUtils';
import { StartRequestCards } from './StartRequestCards';

// The approval card of a child-session start: the FULL prompt, Start / Don't start, withdrawn by the daemon.

const ask = (prompt = 'Add V43\nthen run the migration tests') => ({
  parent_session: 'a1',
  project_id: 'p1',
  agent: 'claude_code' as const,
  title: 'Write the migration',
  prompt,
  worktree: null,
});

describe('StartRequestCards', () => {
  it('shows nothing without a request', () => {
    mount(<StartRequestCards />);
    expect(screen.queryByRole('dialog')).toBeNull();
  });

  it('shows the full prompt, the parent session and the agent', () => {
    const { client } = mount(<StartRequestCards />);
    const long = `${'x'.repeat(3000)}\nlast line`;
    act(() => void client.requestSessionStart(ask(long)));
    const dialog = screen.getByRole('dialog', { name: /start a session/i });
    expect(dialog.querySelector('[data-part="prompt"]')?.textContent).toBe(long);
    expect(dialog).toHaveTextContent('Write the migration');
    expect(dialog).toHaveTextContent('Claude Code');
  });

  it('Start approves: the child exists under its parent and the card closes', async () => {
    const { client } = mount(<StartRequestCards />);
    const before = (await client.listSessions()).length;
    act(() => void client.requestSessionStart(ask()));
    await act(async () => {
      fireEvent.click(screen.getByRole('button', { name: 'Start session' }));
    });
    const after = await client.listSessions();
    expect(after).toHaveLength(before + 1);
    expect(after.at(-1)?.parent_session).toBe('a1');
    expect(screen.queryByRole('dialog')).toBeNull();
  });

  it('Dismiss declines: nothing starts and the card closes; Esc dismisses too', async () => {
    const { client } = mount(<StartRequestCards />);
    const before = (await client.listSessions()).length;
    act(() => void client.requestSessionStart(ask()));
    await act(async () => {
      fireEvent.click(screen.getByRole('button', { name: 'Dismiss' }));
    });
    expect(await client.listSessions()).toHaveLength(before);
    expect(screen.queryByRole('dialog')).toBeNull();
    act(() => void client.requestSessionStart(ask()));
    await act(async () => {
      fireEvent.keyDown(window, { key: 'Escape' });
    });
    expect(screen.queryByRole('dialog')).toBeNull();
  });

  it('a withdrawn request disappears without an answer', () => {
    const { client } = mount(<StartRequestCards />);
    let id = '';
    act(() => {
      id = client.requestSessionStart(ask());
    });
    expect(screen.getByRole('dialog')).toBeInTheDocument();
    act(() => client.withdrawSessionStart(id));
    expect(screen.queryByRole('dialog')).toBeNull();
  });

  it('queues several requests: the oldest first, with the count', async () => {
    const { client } = mount(<StartRequestCards />);
    act(() => {
      client.requestSessionStart(ask('first'));
      client.requestSessionStart(ask('second'));
    });
    expect(screen.getByRole('dialog').querySelector('[data-part="prompt"]')).toHaveTextContent('first');
    expect(screen.getByRole('dialog')).toHaveTextContent('1 of 2');
    await act(async () => {
      fireEvent.click(screen.getByRole('button', { name: 'Dismiss' }));
    });
    expect(screen.getByRole('dialog').querySelector('[data-part="prompt"]')).toHaveTextContent('second');
  });

  it('a failed start keeps the card and says why', async () => {
    const client = new MockClient({ script: false });
    mount(<StartRequestCards />, client);
    act(() => void client.requestSessionStart({ ...ask(), project_id: 'gone' }));
    await act(async () => {
      fireEvent.click(screen.getByRole('button', { name: 'Start session' }));
    });
    expect(screen.getByRole('alert')).toBeInTheDocument();
    expect(screen.getByRole('dialog')).toBeInTheDocument();
  });
});
