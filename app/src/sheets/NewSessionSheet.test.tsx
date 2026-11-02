import { act, fireEvent, screen, waitFor } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { describe, expect, it, vi } from 'vitest';
import { emitUiCommand } from '../lib/ui/commands';
import { getSelectedSessionId, selectSession } from '../lib/ui/selection';
import { mountWp5 } from './harness';
import { getOverlay, openOverlay } from './overlay';

const open = async () => {
  act(() => void emitUiCommand('openNewSession'));
  return screen.findByRole('dialog', { name: 'New session' });
};

describe('New session sheet', () => {
  it('opens on the UI command, defaulting to Terminal in the current session\'s project', async () => {
    const { client } = mountWp5();
    await client.getSnapshot();
    selectSession('a3');
    await open();
    expect(screen.getByRole('button', { name: 'Terminal' })).toHaveAttribute('aria-pressed', 'true');
    await waitFor(() => expect(screen.getByLabelText('Project')).toHaveValue('p2'));
    expect(screen.getByTestId('cmd-hint')).toHaveTextContent('Runs $SHELL -l');
    expect(screen.queryByLabelText(/First prompt/)).toBeNull();
    expect(screen.queryByLabelText('Worktree branch name')).toBeNull();
    fireEvent.keyDown(window, { key: 'Escape' });
    expect(getOverlay()).toBeNull();
  });

  it('preselects the project it was opened for, not the current session\'s, and follows a newer request', async () => {
    const { client } = mountWp5();
    await client.getSnapshot();
    selectSession('a1'); // billing-api (p1)
    act(() => void emitUiCommand('openNewSession', 'p2'));
    await screen.findByRole('dialog', { name: 'New session' });
    await waitFor(() => expect(screen.getByLabelText('Project')).toHaveValue('p2'));
    act(() => void emitUiCommand('openNewSession', 'p3')); // asked again for another project while it is open
    await waitFor(() => expect(screen.getByLabelText('Project')).toHaveValue('p3'));
  });

  it('shows the configured command and the first prompt for agents only', async () => {
    mountWp5();
    await open();
    await userEvent.click(screen.getByRole('button', { name: 'Claude Code' }));
    expect(screen.getByTestId('cmd-hint')).toHaveTextContent('Runs claude');
    expect(screen.getByLabelText(/First prompt/)).toBeInTheDocument();
    await userEvent.click(screen.getByRole('button', { name: 'Terminal' }));
    expect(screen.queryByLabelText(/First prompt/)).toBeNull();
  });

  it('validates the worktree branch and blocks Start until it is valid', async () => {
    mountWp5();
    await open();
    await userEvent.click(await screen.findByRole('checkbox', { name: /worktree/ }));
    const branch = screen.getByLabelText('Worktree branch name');
    expect(branch).toHaveValue('lotse/new-task');
    expect(screen.getByRole('button', { name: /Start/ })).toBeEnabled();
    await userEvent.clear(branch);
    expect(screen.getByRole('alert')).toHaveTextContent('Branch name is required');
    expect(screen.getByRole('button', { name: /Start/ })).toBeDisabled();
    await userEvent.type(branch, 'has space');
    expect(screen.getByRole('alert')).toHaveTextContent(/invalid/);
    expect(screen.getByRole('button', { name: /Start/ })).toBeDisabled();
    await userEvent.clear(branch);
    await userEvent.type(branch, 'lotse/ok');
    expect(screen.queryByRole('alert')).toBeNull();
    expect(screen.getByRole('button', { name: /Start/ })).toBeEnabled();
  });

  it('disables the worktree option for a project without git', async () => {
    mountWp5();
    await open();
    await userEvent.selectOptions(await screen.findByLabelText('Project'), 'notes');
    expect(screen.getByRole('checkbox', { name: /worktree/ })).toBeDisabled();
    expect(screen.getByText('(No git)')).toBeInTheDocument();
  });

  it('a ticked worktree does not block a project without git (switching project after ticking)', async () => {
    const { client } = mountWp5();
    const create = vi.spyOn(client, 'createSession');
    await open();
    await userEvent.click(await screen.findByRole('checkbox', { name: /worktree/ }));
    await userEvent.selectOptions(screen.getByLabelText('Project'), 'notes');
    expect(screen.getByRole('checkbox', { name: /worktree/ })).toBeDisabled();
    expect(screen.queryByLabelText('Worktree branch name')).toBeNull();
    await userEvent.click(screen.getByRole('button', { name: /Start/ }));
    await waitFor(() => expect(create).toHaveBeenCalled());
    expect(create.mock.calls[0]?.[0]).toMatchObject({ project_id: 'p5', worktree: null });
  });

  it('opens with the project the palette asked for', async () => {
    mountWp5();
    act(() => openOverlay({ name: 'newSession', projectId: 'p3' }));
    await waitFor(() => expect(screen.getByLabelText('Project')).toHaveValue('p3'));
  });

  it('creates the session with the form values and selects it', async () => {
    const { client } = mountWp5();
    const create = vi.spyOn(client, 'createSession');
    await open();
    await userEvent.selectOptions(await screen.findByLabelText('Project'), 'billing-api');
    await userEvent.click(screen.getByRole('button', { name: 'Codex' }));
    await userEvent.click(screen.getByRole('checkbox', { name: /worktree/ }));
    await userEvent.type(screen.getByLabelText(/First prompt/), '  fix the build  ');
    await userEvent.click(screen.getByRole('button', { name: /Start/ }));
    await waitFor(() => expect(create).toHaveBeenCalledTimes(1));
    expect(create).toHaveBeenCalledWith({
      project_id: 'p1',
      agent: 'codex',
      command: null,
      title: null,
      worktree: { branch: 'lotse/new-task' },
      first_prompt: 'fix the build',
    });
    await waitFor(() => expect(getOverlay()).toBeNull());
    const created = (await client.listSessions()).find((s) => s.agent === 'codex' && s.git.worktree && s.project_id === 'p1' && s.id !== 'a2');
    expect(getSelectedSessionId()).toBe(created?.id);
  });

  it('a slow createSession does not close an overlay opened meanwhile; opening it twice still closes after submit', async () => {
    const { client } = mountWp5();
    const real = client.createSession.bind(client);
    let release: () => void = () => {};
    const gate = new Promise<void>((r) => (release = r));
    vi.spyOn(client, 'createSession').mockImplementation(async (n) => {
      await gate;
      return real(n);
    });
    await open();
    act(() => openOverlay({ name: 'newSession' })); // opened again while open: same sheet, new entry
    await screen.findByLabelText('Project');
    await userEvent.click(screen.getByRole('button', { name: /Start/ }));
    act(() => openOverlay({ name: 'shortcuts' }));
    release();
    await waitFor(() => expect(getSelectedSessionId()).not.toBeNull());
    expect(getOverlay()?.name).toBe('shortcuts');

    // and without an overlay in between, the same slow job does close its own sheet
    act(() => openOverlay({ name: 'newSession' }));
    await screen.findByLabelText('Project');
    await userEvent.click(screen.getByRole('button', { name: /Start/ }));
    await waitFor(() => expect(getOverlay()).toBeNull());
  });

  it('starts a plain Terminal without worktree or prompt', async () => {
    const { client } = mountWp5();
    const create = vi.spyOn(client, 'createSession');
    await open();
    await screen.findByLabelText('Project');
    await userEvent.click(screen.getByRole('button', { name: /Start/ }));
    await waitFor(() => expect(create).toHaveBeenCalled());
    expect(create.mock.calls[0]?.[0]).toMatchObject({ agent: 'terminal', worktree: null, first_prompt: null });
  });

  it('shows the daemon error and stays open', async () => {
    const { client } = mountWp5();
    vi.spyOn(client, 'createSession').mockRejectedValue(new Error('daemon said no'));
    await open();
    await screen.findByLabelText('Project');
    await userEvent.click(screen.getByRole('button', { name: /Start/ }));
    expect(await screen.findByRole('alert')).toHaveTextContent('daemon said no');
    expect(getOverlay()?.name).toBe('newSession');
    expect(screen.getByRole('button', { name: /Start/ })).toBeEnabled();
  });

  it('"Change…" jumps to Settings › Agents', async () => {
    mountWp5();
    await open();
    await userEvent.click(screen.getByRole('button', { name: 'Change…' }));
    expect(getOverlay()).toEqual({ name: 'settings', section: 'Agents' });
    expect(await screen.findByRole('dialog', { name: 'Settings' })).toBeInTheDocument();
  });
});
