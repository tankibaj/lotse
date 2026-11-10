import { act, fireEvent, screen, waitFor, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { afterEach, describe, expect, it, vi } from 'vitest';
import type { Session } from '../lib/client/types';
import { emitUiCommand, onUiCommand } from '../lib/ui/commands';
import type { UiCommand } from '../lib/ui/commands';
import { closePanel, getOpenPanel } from '../lib/ui/panel';
import { SHORTCUTS } from '../sheets/ShortcutsSheet';
import { getSelectedSessionId, selectSession } from '../lib/ui/selection';
import { mountWp5 } from '../sheets/harness';
import { onSplitRequest } from '../panes/splitBus';
import { getOverlay } from '../sheets/overlay';
import { COMMANDS, NO_SHORTCUT, UI_COMMAND_ITEM, nextNeedingYou, paletteItems } from './items';

const open = async () => {
  act(() => void emitUiCommand('openPalette'));
  await screen.findByRole('dialog', { name: 'Command palette' });
  await screen.findAllByRole('option');
  return screen.getByRole('combobox', { name: 'Command palette' });
};
const labels = () => screen.getAllByRole('option').map((o) => o.textContent ?? '');

afterEach(() => closePanel());

describe('Command palette', () => {
  it('opens on the command and ⌘K (which also closes it), focused, with sessions most urgent first', async () => {
    mountWp5();
    const input = await open();
    expect(input).toHaveFocus();
    expect(input).toHaveAttribute('placeholder', 'Jump to session, project or command…');
    const first = labels();
    expect(first[0]).toContain('praxis-web › Patient list → Signals'); // perm
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
    for (const s of ['billing-api › Fix invoice rounding', 'notes › Scratch shell', 'Next session that needs you', 'Add a Project…', 'New session…', 'Settings…', 'Pause notifications']) {
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

  it('has a command for each panel, the composer, the review and the transcript / changes sheets, with their shortcuts', async () => {
    mountWp5();
    await open();
    const row = (name: string) => screen.getAllByRole('option').find((o) => o.textContent?.includes(name));
    expect(row('Session panel')).toHaveTextContent('⇧⌘I');
    expect(row('Changes panel')).toHaveTextContent('⇧⌘G');
    expect(row('Files panel')).toHaveTextContent('⇧⌘F');
    expect(row('Transcript panel')).toHaveTextContent('⇧⌘T');
    expect(row('Usage & time panel')).toHaveTextContent('⇧⌘U');
    expect(row('Open composer')).toHaveTextContent('⌘⇧↩');
    expect(row('Open review')).toHaveTextContent('⇧⌘R');
    expect(row('Show transcript')).toBeDefined();
    expect(row('Show changes')).toBeDefined();
  });

  it('panel commands toggle that panel; composer, review and the sheets act on the selected session', async () => {
    mountWp5();
    selectSession('a2');
    const got: unknown[] = [];
    const offs = (['togglePanel', 'openComposer', 'openReview'] as const).map((c) => onUiCommand(c, (arg) => got.push([c, arg])));
    for (const [query, want] of [
      ['changes panel', ['togglePanel', 'changes']],
      ['usage', ['togglePanel', 'usage']],
      ['open composer', ['openComposer', undefined]],
      ['open review', ['openReview', 'a2']],
    ] as const) {
      got.length = 0;
      act(() => void emitUiCommand('openPalette'));
      await userEvent.type(await screen.findByRole('combobox'), `${query}{Enter}`);
      expect(got, query).toEqual([want]);
      expect(getOverlay()).toBeNull();
    }
    offs.forEach((off) => off());
  });

  it('Show transcript / Show changes open the panel for the selected session, once, and no sheet', async () => {
    mountWp5();
    selectSession('a2');
    const got: unknown[] = [];
    const offs = (['showTranscript', 'showChanges'] as const).map((c) => onUiCommand(c, () => got.push(c)));
    for (const [query, panel] of [['show transcript', 'transcript'], ['show changes', 'changes']] as const) {
      closePanel();
      act(() => void emitUiCommand('openPalette'));
      await userEvent.type(await screen.findByRole('combobox'), `${query}{Enter}`);
      expect(getOpenPanel(), query).toBe(panel);
      expect(getOverlay(), query).toBeNull(); // Sheets.tsx opens its sheet on the showX events: the palette must not raise them
    }
    expect(got).toEqual([]);
    offs.forEach((off) => off());
  });

  it('review, transcript and changes commands do nothing without a selected session', async () => {
    mountWp5();
    const got: unknown[] = [];
    const offs = (['openReview'] as const).map((c) => onUiCommand(c, () => got.push(c)));
    await new Promise((r) => setTimeout(r, 50)); // the sidebar's initial selection lands with the snapshot
    act(() => selectSession(null));
    for (const query of ['open review', 'show transcript', 'show changes']) {
      act(() => void emitUiCommand('openPalette'));
      await userEvent.type(await screen.findByRole('combobox'), `${query}{Enter}`);
      act(() => selectSession(null)); // a session item may match the query and take the selection
    }
    expect(got).toEqual([]);
    expect(getOpenPanel()).toBeNull();
    expect(getOverlay()).toBeNull();
    offs.forEach((off) => off());
  });

  it('session rows start with the project name, and typing the project name filters to its sessions', async () => {
    mountWp5();
    const input = await open();
    expect(labels().some((l) => l.includes('billing-api › '))).toBe(true);
    await userEvent.type(input, 'billing-api');
    const rows = labels().filter((l) => l.includes(' › '));
    expect(rows.length).toBeGreaterThan(0);
    expect(rows.every((l) => l.includes('billing-api › '))).toBe(true);
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

describe('every UiCommand is in the palette with a shortcut (WP20)', () => {
  const sheetKeys = SHORTCUTS.map(([, keys]) => keys).join(' ');

  it.each(Object.entries(UI_COMMAND_ITEM))('%s', (command, id) => {
    if (id === null) {
      expect(command).toBe('openPalette'); // the palette itself
      expect(sheetKeys).toContain('⌘K');
      return;
    }
    const entry = COMMANDS.find((c) => c.id === id);
    expect(entry, `${command}: no palette item ${id}`).toBeDefined();
    if (NO_SHORTCUT.includes(command as UiCommand)) {
      expect(entry?.hint).toBe('');
      return;
    }
    expect(entry?.hint, `${command}: no shortcut`).not.toBe('');
    expect(sheetKeys, `${command}: ${entry?.hint} missing from the Shortcuts sheet`).toContain(entry?.hint);
  });

  it('only the commands with no keyboard shortcut yet are exempt', () => {
    expect([...NO_SHORTCUT].sort()).toEqual(['openNewSession', 'openProjectStyle']);
  });

  it('every palette command with a shortcut hint is on the Shortcuts sheet', () => {
    for (const c of COMMANDS.filter((x) => x.hint)) expect(sheetKeys, c.label).toContain(c.hint);
  });
});

describe('Beta actions in the palette (WP20)', () => {
  const run = async (query: string) => {
    act(() => void emitUiCommand('openPalette'));
    await userEvent.type(await screen.findByRole('combobox'), `${query}{Enter}`);
  };
  afterEach(() => closePanel());

  it('lists focus, close, new session, sidebar, project style and export with their shortcuts', async () => {
    mountWp5();
    await open();
    const row = (name: string) => screen.getAllByRole('option').find((o) => o.textContent?.includes(name));
    expect(row('Focus previous pane')).toHaveTextContent('⌘[');
    expect(row('Focus next pane')).toHaveTextContent('⌘]');
    expect(row('Close pane')).toHaveTextContent('⌘W');
    expect(row('New terminal')).toHaveTextContent('⌘T');
    expect(row('New Claude Code session')).toHaveTextContent('⌥⌘T');
    expect(row('Toggle sidebar')).toHaveTextContent('⌃⌘S');
    expect(row('Export transcript')).toBeDefined();
    expect(row('Project colour & icon')).toBeDefined();
  });

  it('focus and close raise UiCommands for Panes, not synthetic key events', async () => {
    mountWp5();
    const pressed: string[] = [];
    const on = (e: KeyboardEvent) => e.metaKey && pressed.push(e.key);
    addEventListener('keydown', on);
    const got: unknown[] = [];
    const offs = (['focusPane', 'closePane'] as const).map((c) => onUiCommand(c, (arg) => got.push([c, arg])));
    await run('focus previous');
    await run('focus next');
    await run('close pane');
    removeEventListener('keydown', on);
    offs.forEach((off) => off());
    expect(got).toEqual([['focusPane', 'prev'], ['focusPane', 'next'], ['closePane', undefined]]);
    expect(pressed).toEqual([]);
  });

  it('hints: only real chords; show / export have none, the panels carry ⇧⌘G / ⇧⌘T', () => {
    const hint = (id: string) => COMMANDS.find((c) => c.id === id)?.hint;
    expect(hint('show-changes')).toBe('');
    expect(hint('show-transcript')).toBe('');
    expect(hint('export-transcript')).toBe('');
    expect(hint('panel-changes')).toBe('⇧⌘G');
    expect(hint('panel-transcript')).toBe('⇧⌘T');
    expect(UI_COMMAND_ITEM.showChanges).toBe('panel-changes');
    expect(UI_COMMAND_ITEM.showTranscript).toBe('panel-transcript');
  });

  it('new terminal and new Claude Code session start a session in the selected project', async () => {
    const { client } = mountWp5();
    selectSession('a2');
    const create = vi.spyOn(client, 'createSession');
    await run('new terminal');
    await waitFor(() => expect(create).toHaveBeenCalledTimes(1));
    await run('new claude code session');
    await waitFor(() => expect(create).toHaveBeenCalledTimes(2));
    expect(create.mock.calls.map(([r]) => r.agent)).toEqual(['terminal', 'claude_code']);
  });

  it('toggle sidebar, project style and export raise their commands', async () => {
    mountWp5();
    selectSession('a2');
    const got: unknown[] = [];
    const offs = (['toggleSidebar', 'openProjectStyle'] as const).map((c) => onUiCommand(c, (arg) => got.push([c, arg])));
    await run('toggle sidebar');
    await run('project colour');
    offs.forEach((off) => off());
    expect(got).toEqual([['toggleSidebar', undefined], ['openProjectStyle', 'p1']]);
    await run('export transcript');
    expect(getOpenPanel()).toBe('transcript');
    await run('export transcript');
    expect(getOpenPanel()).toBe('transcript'); // opens, never toggles it shut
  });

  it('showChanges / showTranscript (session menu, palette) select the session and open that panel', () => {
    mountWp5();
    act(() => void emitUiCommand('showChanges', 'a2'));
    expect(getSelectedSessionId()).toBe('a2');
    expect(getOpenPanel()).toBe('changes');
    act(() => void emitUiCommand('showChanges', 'a2'));
    expect(getOpenPanel()).toBe('changes');
    act(() => void emitUiCommand('showTranscript', 'a1'));
    expect(getSelectedSessionId()).toBe('a1');
    expect(getOpenPanel()).toBe('transcript');
  });
});

describe('Shortcuts sheet = mock #keys (WP20)', () => {
  const sheet = (what: string) => SHORTCUTS.find(([w]) => w === what)?.[1];

  it('uses the mock wording, with Review on ⇧⌘R and the panels on ⇧⌘G F T U I', () => {
    expect(sheet('New terminal')).toBe('⌘T'); // ⌘T always starts a terminal (shortcuts.ts)
    expect(sheet('Full diff review')).toBe('⇧⌘R');
    expect(sheet('Changes · Files · Transcript · Usage · Session')).toBe('⇧⌘G · ⇧⌘F · ⇧⌘T · ⇧⌘U · ⇧⌘I');
    expect(SHORTCUTS.map(([, k]) => k).join(' ')).not.toContain('⌥⌘D');
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
