import { act, fireEvent, screen } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { describe, expect, it, vi } from 'vitest';
import { mountWp5 } from './harness';
import { setHostBridge } from './hostBridge';
import type { HostBridge } from './hostBridge';
import { getOverlay, openOverlay } from './overlay';

const key = (init: KeyboardEventInit) => fireEvent.keyDown(window, init);

describe('shortcut bindings', () => {
  it('⌘/ toggles the shortcuts sheet, which lists the shortcuts', async () => {
    mountWp5();
    key({ code: 'Slash', metaKey: true });
    const dlg = await screen.findByRole('dialog', { name: 'Keyboard shortcuts' });
    expect(dlg).toHaveTextContent('Command palette / jump anywhere');
    expect(dlg).toHaveTextContent('⇧⌘O');
    key({ code: 'Slash', metaKey: true });
    expect(getOverlay()).toBeNull();
  });

  it('the shortcuts sheet has a Close button and Tab cannot leave it', async () => {
    mountWp5();
    const outside = document.createElement('button');
    document.body.append(outside);
    key({ code: 'Slash', metaKey: true });
    const dlg = await screen.findByRole('dialog', { name: 'Keyboard shortcuts' });
    expect(screen.getByRole('button', { name: 'Close' })).toHaveFocus();
    await userEvent.tab();
    await userEvent.tab({ shift: true });
    expect(dlg.contains(document.activeElement)).toBe(true);
    await userEvent.click(screen.getByRole('button', { name: 'Close' }));
    expect(getOverlay()).toBeNull();
    outside.remove();
  });

  it('⌘/ also works where "/" is ⇧7 (German layout)', () => {
    mountWp5();
    key({ code: 'Digit7', key: '/', shiftKey: true, metaKey: true });
    expect(getOverlay()?.name).toBe('shortcuts');
  });

  it('⌘T, ⇧⌘O, ⌘, and ⌘K each open their overlay, replacing the one before', async () => {
    mountWp5();
    key({ code: 'KeyT', metaKey: true });
    expect(getOverlay()?.name).toBe('newSession');
    key({ code: 'KeyO', metaKey: true, shiftKey: true });
    expect(getOverlay()?.name).toBe('addProject');
    key({ code: 'Comma', metaKey: true });
    expect(getOverlay()?.name).toBe('settings');
    key({ code: 'KeyK', metaKey: true });
    expect(getOverlay()?.name).toBe('palette');
    expect(screen.getAllByRole('dialog')).toHaveLength(1);
  });

  it('ignores the keys without ⌘ and with other modifiers', () => {
    mountWp5();
    key({ code: 'KeyT' });
    key({ code: 'KeyT', metaKey: true, shiftKey: true }); // ⇧⌘T is the Transcript panel (Beta)
    key({ code: 'KeyT', metaKey: true, altKey: true });
    key({ code: 'KeyO', metaKey: true });
    key({ code: 'KeyK', metaKey: true, shiftKey: true });
    expect(getOverlay()).toBeNull();
  });

  it('traps Tab inside the open sheet and restores focus on close', async () => {
    mountWp5();
    const trigger = document.createElement('button');
    document.body.append(trigger);
    trigger.focus();
    key({ code: 'KeyT', metaKey: true });
    const dlg = await screen.findByRole('dialog');
    await screen.findByLabelText('Project');
    for (let i = 0; i < 12; i++) {
      await userEvent.tab();
      expect(dlg.contains(document.activeElement)).toBe(true);
    }
    await userEvent.tab({ shift: true });
    expect(dlg.contains(document.activeElement)).toBe(true);
    key({ key: 'Escape' });
    expect(trigger).toHaveFocus();
    trigger.remove();
  });
});

describe('folder drops on the sidebar', () => {
  const setup = (over: Partial<HostBridge>) => {
    let drop: (p: string[]) => void = () => {};
    const side = document.createElement('div');
    side.dataset.slot = 'sidebar';
    document.body.append(side);
    setHostBridge({
      pickFolder: () => undefined,
      scanRepos: async (path) => ({ is_repo: true, repos: [path] }),
      cloneRepo: async () => {},
      createProject: async () => {},
      watchDrops: (_region, h) => {
        drop = h.onDrop;
        h.onHover(true);
        return () => {};
      },
      ...over,
    });
    return { side, drop: (p: string[]) => drop(p) };
  };

  it('adds a dropped repo straight away and outlines the sidebar while hovering', async () => {
    const { side, drop } = setup({});
    const { client } = mountWp5();
    const add = vi.spyOn(client, 'addProject');
    expect(side.style.outline).toContain('dashed');
    await act(async () => drop(['/Users/naim/Repos/dropped']));
    expect(add).toHaveBeenCalledWith({ path: '/Users/naim/Repos/dropped', name: null, color: null, icon: null });
    side.remove();
  });

  it('offers each repo of a dropped multi-repo folder', async () => {
    const { side, drop } = setup({ scanRepos: async () => ({ is_repo: false, repos: ['/m/a', '/m/b'] }) });
    const { client } = mountWp5();
    const add = vi.spyOn(client, 'addProject');
    await act(async () => drop(['/m']));
    expect(add).not.toHaveBeenCalled();
    expect(getOverlay()).toEqual({ name: 'addProject', repos: { root: '/m', repos: ['/m/a', '/m/b'] } });
    expect(await screen.findAllByRole('checkbox')).toHaveLength(2);
    side.remove();
  });

  it('opens the sheet when the drop cannot be added', async () => {
    const { side, drop } = setup({ scanRepos: async () => { throw new Error('not a folder'); } });
    mountWp5();
    await act(async () => drop(['/file.txt']));
    expect(getOverlay()?.name).toBe('addProject');
    side.remove();
  });
});

describe('overlay store', () => {
  it('opening one overlay closes the other', () => {
    openOverlay({ name: 'shortcuts' });
    openOverlay({ name: 'palette' });
    expect(getOverlay()?.name).toBe('palette');
  });
});
