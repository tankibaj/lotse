import { act, screen, waitFor } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { describe, expect, it, vi } from 'vitest';
import { emitUiCommand } from '../lib/ui/commands';
import { mountWp5 } from './harness';
import { setHostBridge } from './hostBridge';
import type { HostBridge } from './hostBridge';
import { getOverlay, openOverlay } from './overlay';

const bridge = (over: Partial<HostBridge> = {}): HostBridge => ({
  pickFolder: () => undefined,
  scanRepos: async (path) => ({ is_repo: true, repos: [path] }),
  cloneRepo: async () => {},
  createProject: async () => {},
  watchDrops: () => () => {},
  ...over,
});

const open = async () => {
  act(() => void emitUiCommand('openAddProject'));
  return screen.findByRole('dialog', { name: 'Add a project' });
};

describe('Add project sheet', () => {
  it('lists the three ways to add, and the drag tip', async () => {
    mountWp5();
    await open();
    expect(screen.getByRole('button', { name: /Browse folder/ })).toBeInTheDocument();
    expect(screen.getByRole('button', { name: /Clone from URL/ })).toBeInTheDocument();
    expect(screen.getByRole('button', { name: /Create new project/ })).toBeInTheDocument();
    expect(screen.getByText(/drag a folder from Finder/)).toBeInTheDocument();
  });

  it('Browse uses the native picker and adds the chosen repo', async () => {
    setHostBridge(bridge({ pickFolder: async () => '/Users/naim/Repos/new-one' }));
    const { client } = mountWp5();
    const add = vi.spyOn(client, 'addProject');
    await open();
    await userEvent.click(screen.getByRole('button', { name: /Browse folder/ }));
    await waitFor(() => expect(add).toHaveBeenCalledWith({ path: '/Users/naim/Repos/new-one', name: null, color: null, icon: null }));
    await waitFor(() => expect(getOverlay()).toBeNull());
  });

  it('a cancelled picker leaves the sheet as it was', async () => {
    setHostBridge(bridge({ pickFolder: async () => null }));
    const { client } = mountWp5();
    const add = vi.spyOn(client, 'addProject');
    await open();
    await userEvent.click(screen.getByRole('button', { name: /Browse folder/ }));
    await waitFor(() => expect(screen.getByRole('button', { name: /Browse folder/ })).toBeInTheDocument());
    expect(add).not.toHaveBeenCalled();
    expect(getOverlay()?.name).toBe('addProject');
  });

  it('without a native picker, Browse falls back to the path field and validates it', async () => {
    const { client } = mountWp5();
    const add = vi.spyOn(client, 'addProject');
    await open();
    await userEvent.click(screen.getByRole('button', { name: /Browse folder/ }));
    const input = screen.getByLabelText('Folder path');
    expect(input).toHaveValue('~/Repos/');
    await userEvent.clear(input);
    await userEvent.click(screen.getByRole('button', { name: /Add/ }));
    expect(screen.getByRole('alert')).toHaveTextContent('Folder path is required');
    await userEvent.type(input, 'relative/dir');
    await userEvent.click(screen.getByRole('button', { name: /Add/ }));
    expect(screen.getByRole('alert')).toHaveTextContent(/must start with/);
    await userEvent.clear(input);
    await userEvent.type(input, '/Users/naim/Repos/billing-api/');
    await userEvent.click(screen.getByRole('button', { name: /Add/ }));
    expect(screen.getByRole('alert')).toHaveTextContent(/already a project/);
    expect(add).not.toHaveBeenCalled();
    await userEvent.clear(input);
    await userEvent.type(input, '/Users/naim/Repos/fresh{Enter}');
    await waitFor(() => expect(add).toHaveBeenCalledWith(expect.objectContaining({ path: '/Users/naim/Repos/fresh' })));
  });

  it('a folder with several repos offers each; only the ticked ones are added', async () => {
    const repos = ['/w/a', '/w/b', '/w/c'];
    setHostBridge(bridge({ pickFolder: async () => '/w', scanRepos: async () => ({ is_repo: false, repos }) }));
    const { client } = mountWp5();
    const add = vi.spyOn(client, 'addProject');
    await open();
    await userEvent.click(screen.getByRole('button', { name: /Browse folder/ }));
    const boxes = await screen.findAllByRole('checkbox');
    expect(boxes).toHaveLength(3);
    expect(boxes.every((b) => (b as HTMLInputElement).checked)).toBe(true);
    await userEvent.click(boxes[1] as HTMLElement);
    await userEvent.click(screen.getByRole('button', { name: 'Add 2 projects ⏎' }));
    await waitFor(() => expect(add).toHaveBeenCalledTimes(2));
    expect(add.mock.calls.map((c) => c[0].path)).toEqual(['/w/a', '/w/c']);
  });

  it('a preset multi-repo list leaves out repos that are already projects', async () => {
    openOverlay({ name: 'addProject', repos: { root: '/Users/naim/Repos', repos: ['/Users/naim/Repos/billing-api', '/Users/naim/Repos/fresh-a', '/Users/naim/Repos/fresh-b'] } });
    const { client } = mountWp5();
    const add = vi.spyOn(client, 'addProject');
    const boxes = await screen.findAllByRole('checkbox');
    await waitFor(() => expect(boxes[0]).toBeDisabled());
    expect(boxes[0]).not.toBeChecked();
    expect(screen.getByText('already added')).toBeInTheDocument();
    await userEvent.click(screen.getByRole('button', { name: 'Add 2 projects ⏎' }));
    await waitFor(() => expect(add).toHaveBeenCalledTimes(2));
    expect(add.mock.calls.map((c) => c[0].path)).toEqual(['/Users/naim/Repos/fresh-a', '/Users/naim/Repos/fresh-b']);
  });

  it('rejects clone URLs that read as git options or yield an unusable folder name', async () => {
    mountWp5();
    await open();
    await userEvent.click(screen.getByRole('button', { name: /Clone from URL/ }));
    await userEvent.type(screen.getByLabelText('Repository URL'), '-ofoo@host:x/y{Enter}');
    expect(screen.getByRole('alert')).toHaveTextContent(/must not start with -/);
    await userEvent.clear(screen.getByLabelText('Repository URL'));
    await userEvent.type(screen.getByLabelText('Repository URL'), 'https://h/..');
    expect(screen.getByRole('alert')).toHaveTextContent('Pick a different name');
  });

  it('cannot add nothing from the repo list', async () => {
    openOverlay({ name: 'addProject', repos: { root: '/w', repos: ['/w/a', '/w/b'] } });
    mountWp5();
    for (const b of await screen.findAllByRole('checkbox')) await userEvent.click(b);
    expect(screen.getByRole('button', { name: /Add 0 projects/ })).toBeDisabled();
  });

  it('Clone validates URL and target, then clones into <dir>/<repo> and adds it', async () => {
    const cloneRepo = vi.fn(async () => {});
    setHostBridge(bridge({ cloneRepo }));
    const { client } = mountWp5();
    const add = vi.spyOn(client, 'addProject');
    await open();
    await userEvent.click(screen.getByRole('button', { name: /Clone from URL/ }));
    await userEvent.click(screen.getByRole('button', { name: /Clone ⏎/ }));
    expect(screen.getByRole('alert')).toHaveTextContent('Repository URL is required');
    await userEvent.type(screen.getByLabelText('Repository URL'), 'not-a-url');
    expect(screen.getByRole('alert')).toHaveTextContent(/Use https/);
    await userEvent.clear(screen.getByLabelText('Repository URL'));
    await userEvent.type(screen.getByLabelText('Repository URL'), 'https://github.com/owner/widget.git');
    await userEvent.clear(screen.getByLabelText('Clone into'));
    await userEvent.type(screen.getByLabelText('Clone into'), 'nowhere');
    await userEvent.click(screen.getByRole('button', { name: /Clone ⏎/ }));
    expect(screen.getByRole('alert')).toHaveTextContent(/must start with/);
    expect(cloneRepo).not.toHaveBeenCalled();
    await userEvent.clear(screen.getByLabelText('Clone into'));
    await userEvent.type(screen.getByLabelText('Clone into'), '~/Repos');
    await userEvent.click(screen.getByRole('button', { name: /Clone ⏎/ }));
    await waitFor(() => expect(cloneRepo).toHaveBeenCalledWith('https://github.com/owner/widget.git', '~/Repos/widget'));
    await waitFor(() => expect(add).toHaveBeenCalledWith(expect.objectContaining({ path: '~/Repos/widget' })));
  });

  it('shows a failing clone and does not add the project', async () => {
    setHostBridge(bridge({ cloneRepo: async () => { throw new Error('fatal: repository not found'); } }));
    const { client } = mountWp5();
    const add = vi.spyOn(client, 'addProject');
    await open();
    await userEvent.click(screen.getByRole('button', { name: /Clone from URL/ }));
    await userEvent.type(screen.getByLabelText('Repository URL'), 'git@github.com:o/r.git{Enter}');
    expect(await screen.findByRole('alert')).toHaveTextContent('fatal: repository not found');
    expect(add).not.toHaveBeenCalled();
    expect(getOverlay()?.name).toBe('addProject');
  });

  it('a slow clone does not close an overlay opened meanwhile', async () => {
    let release: () => void = () => {};
    const gate = new Promise<void>((r) => (release = r));
    setHostBridge(bridge({ cloneRepo: () => gate }));
    const { client } = mountWp5();
    const add = vi.spyOn(client, 'addProject');
    await open();
    await userEvent.click(screen.getByRole('button', { name: /Clone from URL/ }));
    await userEvent.type(screen.getByLabelText('Repository URL'), 'https://github.com/o/slow.git{Enter}');
    act(() => openOverlay({ name: 'shortcuts' }));
    release();
    await waitFor(() => expect(add).toHaveBeenCalledWith(expect.objectContaining({ path: '~/Repos/slow' })));
    expect(getOverlay()?.name).toBe('shortcuts');
  });

  it('Create needs a valid name and location; git init follows the checkbox', async () => {
    const createProject = vi.fn(async () => {});
    setHostBridge(bridge({ createProject }));
    const { client } = mountWp5();
    const add = vi.spyOn(client, 'addProject');
    await open();
    await userEvent.click(screen.getByRole('button', { name: /Create new project/ }));
    expect(screen.getByRole('checkbox', { name: /Initialize git/ })).toBeChecked();
    await userEvent.click(screen.getByRole('button', { name: /Create ⏎/ }));
    expect(screen.getByRole('alert')).toHaveTextContent('Project name is required');
    await userEvent.type(screen.getByLabelText('Project name'), 'a/b');
    expect(screen.getByRole('alert')).toHaveTextContent(/must not contain/);
    await userEvent.clear(screen.getByLabelText('Project name'));
    await userEvent.type(screen.getByLabelText('Project name'), 'billing-api');
    await userEvent.clear(screen.getByLabelText('Location'));
    await userEvent.type(screen.getByLabelText('Location'), '/Users/naim/Repos');
    expect(screen.getByRole('alert')).toHaveTextContent(/already a project/);
    await userEvent.clear(screen.getByLabelText('Project name'));
    await userEvent.type(screen.getByLabelText('Project name'), 'my-project');
    await userEvent.click(screen.getByRole('checkbox', { name: /Initialize git/ }));
    await userEvent.click(screen.getByRole('button', { name: /Create ⏎/ }));
    await waitFor(() => expect(createProject).toHaveBeenCalledWith('/Users/naim/Repos/my-project', false));
    await waitFor(() => expect(add).toHaveBeenCalledWith(expect.objectContaining({ path: '/Users/naim/Repos/my-project' })));
  });

  it('keeps focus inside the dialog across view changes, starting on the first action', async () => {
    mountWp5();
    const dlg = await open();
    expect(screen.getByRole('button', { name: /Browse folder/ })).toHaveFocus();
    await userEvent.click(screen.getByRole('button', { name: /Clone from URL/ }));
    expect(screen.getByLabelText('Repository URL')).toHaveFocus();
    await userEvent.click(screen.getByRole('button', { name: '‹ Back' }));
    expect(dlg.contains(document.activeElement)).toBe(true);
    expect(screen.getByRole('button', { name: /Browse folder/ })).toHaveFocus();
    await userEvent.click(screen.getByRole('button', { name: /Create new project/ }));
    expect(screen.getByLabelText('Project name')).toHaveFocus();
  });

  it('‹ Back returns home; ⇧⌘O opens; Esc closes', async () => {
    mountWp5();
    await open();
    await userEvent.click(screen.getByRole('button', { name: /Create new project/ }));
    await userEvent.click(screen.getByRole('button', { name: '‹ Back' }));
    expect(screen.getByRole('button', { name: /Browse folder/ })).toBeInTheDocument();
    await userEvent.keyboard('{Escape}');
    expect(getOverlay()).toBeNull();
    act(() => void window.dispatchEvent(new KeyboardEvent('keydown', { code: 'KeyO', metaKey: true, shiftKey: true })));
    expect(await screen.findByRole('dialog', { name: 'Add a project' })).toBeInTheDocument();
  });
});
