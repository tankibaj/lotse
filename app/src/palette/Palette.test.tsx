import { act, fireEvent, screen, waitFor, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { describe, expect, it } from 'vitest';
import type { Session } from '../lib/client/types';
import { emitUiCommand } from '../lib/ui/commands';
import { getSelectedSessionId, selectSession } from '../lib/ui/selection';
import { mountWp5 } from '../sheets/harness';
import { onSplitRequest } from '../panes/splitBus';
import { getOverlay } from '../sheets/overlay';
import { nextNeedingYou, paletteItems } from './items';

const open = async () => {
  act(() => void emitUiCommand('openPalette'));
  await screen.findByRole('dialog', { name: 'Command palette' });
  await screen.findAllByRole('option');
  return screen.getByRole('combobox', { name: 'Command palette' });
};
const labels = () => screen.getAllByRole('option').map((o) => o.textContent ?? '');

describe('Command palette', () => {
  it('opens on the command and ⌘K (which also closes it), focused, with sessions most urgent first', async () => {
    mountWp5();
    const input = await open();
    expect(input).toHaveFocus();
    expect(input).toHaveAttribute('placeholder', 'Jump to session, project or command…');
    const first = labels();
    expect(first[0]).toContain('Patient list → Signals · praxis-web'); // perm
    expect(first[1]).toContain('Bump Angular 20'); // fail
    fireEvent.keyDown(window, { code: 'KeyK', metaKey: true });
    expect(getOverlay()).toBeNull();
    fireEvent.keyDown(window, { code: 'KeyK', metaKey: true });
    expect(await screen.findByRole('dialog', { name: 'Command palette' })).toBeInTheDocument();
  });

  it('lists sessions, projects and commands with their shortcuts', async () => {
    mountWp5();
    await open();
    const all = labels().join('\n');
    for (const s of ['Fix invoice rounding · billing-api', 'Scratch shell · notes', 'Next session that needs you', 'Add a Project…', 'New session…', 'Settings…', 'Pause notifications']) {
      expect(all).toContain(s);
    }
    expect(all).toContain('⇧⌘O');
    expect(screen.getAllByRole('option').filter((o) => o.textContent?.endsWith('Project')).length).toBe(5);
  });

  it('Split Right and Split Down commands show their shortcuts and ask the panes for the next session', async () => {
    mountWp5();
    const asked: unknown[] = [];
    const off = onSplitRequest((id, direction) => asked.push([id, direction]));
    const input = await open();
    const row = (name: string) => screen.getAllByRole('option').find((o) => o.textContent?.includes(name));
    expect(row('Split Right')).toHaveTextContent('⌘D');
    expect(row('Split Down')).toHaveTextContent('⇧⌘D');
    await userEvent.type(input, 'split down');
    expect(labels()).toHaveLength(1);
    await userEvent.keyboard('{Enter}');
    expect(asked).toEqual([[null, 'down']]);
    expect(getOverlay()).toBeNull();
    await open();
    await userEvent.click(row('Split Right') as HTMLElement);
    expect(asked).toEqual([[null, 'down'], [null, 'right']]);
    off();
  });

  it('filters by label or hint and reports no matches', async () => {
    mountWp5();
    const input = await open();
    await userEvent.type(input, 'angular');
    expect(labels()).toHaveLength(1);
    await userEvent.clear(input);
    await userEvent.type(input, 'codex');
    expect(labels().every((l) => /Codex/.test(l))).toBe(true);
    await userEvent.clear(input);
    await userEvent.type(input, 'qqqq');
    expect(screen.getByText('No matches')).toBeInTheDocument();
  });

  it('arrow keys move the selection and Enter jumps to the session', async () => {
    mountWp5();
    const input = await open();
    expect(screen.getAllByRole('option')[0]).toHaveAttribute('aria-selected', 'true');
    await userEvent.keyboard('{ArrowDown}');
    expect(screen.getAllByRole('option')[1]).toHaveAttribute('aria-selected', 'true');
    expect(input).toHaveAttribute('aria-activedescendant', screen.getAllByRole('option')[1]?.id);
    await userEvent.keyboard('{ArrowUp}{ArrowUp}');
    expect(screen.getAllByRole('option')[0]).toHaveAttribute('aria-selected', 'true');
    await userEvent.keyboard('{ArrowDown}{Enter}');
    expect(getSelectedSessionId()).toBe('a4');
    expect(getOverlay()).toBeNull();
  });

  it('clicking a session selects it', async () => {
    mountWp5();
    await open();
    await userEvent.click(screen.getByRole('option', { name: /Scratch shell/ }));
    expect(getSelectedSessionId()).toBe('a7');
  });

  it('a project jumps to its latest session', async () => {
    mountWp5();
    const input = await open();
    await userEvent.type(input, 'dotfiles');
    await userEvent.click(within(screen.getByRole('listbox')).getByText('Project').closest('[role=option]') as HTMLElement);
    expect(getSelectedSessionId()).toBe('a6');
  });

  it('a project without sessions opens New session on that project', async () => {
    const { client } = mountWp5();
    const fresh = await client.addProject({ path: '/Users/naim/Repos/empty-one', name: null, color: null, icon: null });
    const input = await open();
    await userEvent.type(input, 'empty-one');
    await userEvent.click(screen.getByRole('option', { name: /empty-one/ }));
    expect(getOverlay()).toEqual({ name: 'newSession', projectId: fresh.id });
    await waitFor(() => expect(screen.getByLabelText('Project')).toHaveValue(fresh.id));
  });

  it('commands open the sheets and Settings', async () => {
    mountWp5();
    let input = await open();
    await userEvent.type(input, 'new session{Enter}');
    expect(getOverlay()?.name).toBe('newSession');
    act(() => void emitUiCommand('openPalette'));
    input = await screen.findByRole('combobox');
    await userEvent.type(input, 'add a project{Enter}');
    expect(getOverlay()?.name).toBe('addProject');
    act(() => void emitUiCommand('openPalette'));
    input = await screen.findByRole('combobox');
    await userEvent.type(input, 'settings{Enter}');
    expect(getOverlay()?.name).toBe('settings');
  });

  it('"Next session that needs you" cycles perm then fail', async () => {
    mountWp5();
    selectSession('a1');
    let input = await open();
    await userEvent.type(input, 'next session{Enter}');
    expect(getSelectedSessionId()).toBe('a3');
    act(() => void emitUiCommand('openPalette'));
    input = await screen.findByRole('combobox');
    await userEvent.type(input, 'next session{Enter}');
    expect(getSelectedSessionId()).toBe('a4');
  });

  it('Pause notifications toggles the setting and the entry flips to Resume', async () => {
    const { client } = mountWp5();
    const input = await open();
    await userEvent.type(input, 'pause{Enter}');
    await waitFor(async () => expect((await client.getSettings()).notifications.paused).toBe(true));
    act(() => void emitUiCommand('openPalette'));
    await userEvent.type(await screen.findByRole('combobox'), 'resume');
    expect(await screen.findByRole('option', { name: /Resume notifications/ })).toBeInTheDocument();
  });

  it('Esc and a click on the backdrop close it', async () => {
    mountWp5();
    await open();
    await userEvent.keyboard('{Escape}');
    expect(getOverlay()).toBeNull();
    await open();
    await userEvent.click(screen.getByTestId('overlay-backdrop'));
    expect(getOverlay()).toBeNull();
  });
});

describe('palette items', () => {
  const s = (id: string, status: Session['status'], at: number): Session =>
    ({ id, project_id: 'p', title: id, agent: 'terminal', status, last_activity: at, git: { is_git: false } }) as Session;

  it('orders sessions perm > fail > run > done > idle, newest first inside a status', () => {
    const items = paletteItems('', [], [s('i', 'idle', 9), s('r1', 'run', 1), s('r2', 'run', 2), s('p', 'perm', 0), s('f', 'fail', 0), s('d', 'done', 0)]);
    expect(items.filter((i) => i.key.startsWith('s:')).map((i) => i.key)).toEqual(['s:p', 's:f', 's:r2', 's:r1', 's:d', 's:i']);
  });

  it('nextNeedingYou skips the current one and wraps around; none when nobody needs you', () => {
    const list = [s('a', 'perm', 3), s('b', 'fail', 2), s('c', 'run', 1)];
    expect(nextNeedingYou(list, null)?.id).toBe('a');
    expect(nextNeedingYou(list, 'a')?.id).toBe('b');
    expect(nextNeedingYou(list, 'b')?.id).toBe('a');
    expect(nextNeedingYou([s('c', 'run', 1)], null)).toBeNull();
  });
});
