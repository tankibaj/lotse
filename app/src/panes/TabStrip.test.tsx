import { act, fireEvent, render, screen, waitFor, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { ClientProvider, MockClient } from '../lib/client';
import { onUiCommand } from '../lib/ui/commands';
import { getSelectedSessionId, selectSession } from '../lib/ui/selection';
import { closeOverlay, getOverlay } from '../sheets/overlay';
import { onTabDrag } from './splitBus';
import type { TabDrag } from './splitBus';
import { TabStrip } from './TabStrip';

let client: MockClient;
const mount = async (tabs = 1) => {
  render(
    <ClientProvider client={client}>
      <TabStrip />
    </ClientProvider>,
  );
  await screen.findAllByRole('tab');
  await waitFor(() => expect(screen.getAllByRole('tab').length).toBeGreaterThanOrEqual(tabs));
};

const tabNames = () => screen.getAllByRole('tab').map((t) => t.querySelector('span.truncate')?.textContent);
const tab = (title: string) => screen.getByRole('tab', { name: new RegExp(title) });

/** A third tab in billing-api (p1): a1, a2 and "Third" (a8). */
const addThird = () => client.createSession({ project_id: 'p1', agent: 'terminal', command: null, title: 'Third', worktree: null, first_prompt: null });

const rect = (left: number, width: number) => ({ left, right: left + width, width, top: 0, bottom: 32, height: 32, x: left, y: 0, toJSON: () => ({}) }) as DOMRect;
/** jsdom has no layout: every tab is 100 px wide, side by side. */
const layout = () => {
  screen.getByRole('tablist').getBoundingClientRect = () => rect(0, 300);
  screen.getAllByRole('tab').forEach((t, i) => (t.getBoundingClientRect = () => rect(i * 100, 100)));
};
const pointer = (type: 'pointermove' | 'pointerup', clientX = 0) => fireEvent(window, new MouseEvent(type, { clientX }));

beforeEach(() => {
  client = new MockClient({ script: false });
  selectSession('a1');
});
afterEach(() => {
  client.close();
  selectSession(null);
  closeOverlay();
  vi.restoreAllMocks();
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

describe('tab context menu', () => {
  it('right-click shows the session menu instead of the WebKit one', async () => {
    await mount();
    const notPrevented = fireEvent.contextMenu(tab('Flyway'), { clientX: 120, clientY: 30 });
    expect(notPrevented).toBe(false);
    const menu = screen.getByRole('menu', { name: 'Flyway migration V42 actions' });
    expect([...menu.querySelectorAll('[role="menuitem"]')].map((i) => i.textContent)).toEqual([
      'Open⏎',
      'Open in Split Right⌥-click',
      'Open in Split Down⌥⇧-click',
      'Rename…',
      'Show Transcript',
      'Show Changes',
      'Stop Session',
    ]);
  });

  it('Open selects the tab, Open in Split Right raises splitSession', async () => {
    await mount();
    const split = vi.fn();
    const off = onUiCommand('splitSession', split);
    fireEvent.contextMenu(tab('Flyway'));
    await userEvent.click(screen.getByRole('menuitem', { name: /^Open(?! in)/ }));
    expect(getSelectedSessionId()).toBe('a2');
    fireEvent.contextMenu(tab('Flyway'));
    await userEvent.click(screen.getByRole('menuitem', { name: /Split Right/ }));
    expect(split).toHaveBeenCalledWith('a2');
    off();
  });

  it('Open in Split Down raises splitSessionDown, not splitSession', async () => {
    await mount();
    const right = vi.fn();
    const down = vi.fn();
    const offs = [onUiCommand('splitSession', right), onUiCommand('splitSessionDown', down)];
    fireEvent.contextMenu(tab('Flyway'));
    await userEvent.click(screen.getByRole('menuitem', { name: /Split Down/ }));
    expect(down).toHaveBeenCalledWith('a2');
    expect(right).not.toHaveBeenCalled();
    offs.forEach((off) => off());
  });

  it('Rename… edits the title in place and renames through the client', async () => {
    await mount();
    const rename = vi.spyOn(client, 'renameSession');
    fireEvent.contextMenu(tab('Flyway'));
    await userEvent.click(screen.getByRole('menuitem', { name: 'Rename…' }));
    const input = screen.getByRole('textbox', { name: 'Session title' });
    expect(screen.getAllByRole('tab')[1]).toContainElement(input);
    await userEvent.clear(input);
    await userEvent.type(input, 'Schema work{Enter}');
    expect(rename).toHaveBeenCalledWith('a2', 'Schema work');
    await waitFor(() => expect(tabNames()).toEqual(['Fix invoice rounding', 'Schema work']));
    expect(screen.queryByRole('textbox')).toBeNull();
  });

  it('Esc cancels a rename without calling the client', async () => {
    await mount();
    const rename = vi.spyOn(client, 'renameSession');
    fireEvent.contextMenu(tab('Flyway'));
    await userEvent.click(screen.getByRole('menuitem', { name: 'Rename…' }));
    await userEvent.type(screen.getByRole('textbox', { name: 'Session title' }), 'x{Escape}');
    expect(rename).not.toHaveBeenCalled();
    expect(tabNames()).toEqual(['Fix invoice rounding', 'Flyway migration V42']);
  });

  it('Stop Session closes an idle session at once', async () => {
    await mount();
    const close = vi.spyOn(client, 'closeSession');
    fireEvent.contextMenu(tab('Flyway'));
    await userEvent.click(screen.getByRole('menuitem', { name: 'Stop Session' }));
    expect(close).toHaveBeenCalledWith('a2');
    await waitFor(() => expect(screen.getAllByRole('tab')).toHaveLength(1));
  });

  it('Stop Session on a working session asks first', async () => {
    await mount();
    const close = vi.spyOn(client, 'closeSession');
    fireEvent.contextMenu(tab('Fix invoice'));
    await userEvent.click(screen.getByRole('menuitem', { name: 'Stop Session' }));
    expect(screen.getByRole('alertdialog')).toHaveTextContent('Stop Fix invoice rounding?');
    expect(close).not.toHaveBeenCalled();
    await userEvent.click(screen.getByRole('button', { name: 'Cancel' }));
    expect(close).not.toHaveBeenCalled();
    fireEvent.contextMenu(tab('Fix invoice'));
    await userEvent.click(screen.getByRole('menuitem', { name: 'Stop Session' }));
    await userEvent.click(screen.getByRole('button', { name: 'Stop' }));
    expect(close).toHaveBeenCalledWith('a1');
  });

  it('Shift+F10 and the ContextMenu key open it on a focused tab', async () => {
    await mount();
    const flyway = tab('Flyway');
    flyway.focus();
    fireEvent.keyDown(flyway, { key: 'F10', shiftKey: true });
    expect(screen.getByRole('menu', { name: 'Flyway migration V42 actions' })).toBeInTheDocument();
    fireEvent.keyDown(screen.getByRole('menu'), { key: 'Escape' });
    expect(screen.queryByRole('menu')).toBeNull();
    fireEvent.keyDown(flyway, { key: 'ContextMenu' });
    expect(screen.getByRole('menu', { name: 'Flyway migration V42 actions' })).toBeInTheDocument();
    expect(screen.getByRole('menuitem', { name: /^Open(?! in)/ })).toHaveFocus();
  });
});

describe('"+" menu', () => {
  const create = () => vi.spyOn(client, 'createSession');
  const openMenu = async () => {
    await userEvent.click(screen.getByRole('button', { name: 'New session' }));
    return screen.getByRole('menu', { name: 'New session' });
  };

  it('lists the terminal, the three agents and the two sheets, with separators and shortcut hints', async () => {
    await mount();
    const menu = await openMenu();
    expect([...menu.querySelectorAll('[role="menuitem"], [role="separator"]')].map((i) => (i.getAttribute('role') === 'separator' ? '—' : i.textContent))).toEqual([
      'New Terminal⌘T',
      '—',
      'Claude Code⌥⌘T',
      'Codex',
      'OpenCode',
      '—',
      'Worktree or other project…',
      'Agent settings…',
    ]);
    expect(menu.querySelectorAll('svg')).toHaveLength(6);
    expect(screen.getByRole('button', { name: 'New session' })).toHaveAttribute('aria-expanded', 'true');
  });

  it('has one active item: the pointer moves the keyboard highlight, there is no separate hover colour', async () => {
    await mount();
    const menu = await openMenu();
    const items = within(menu).getAllByRole('menuitem');
    expect(items[0]).toHaveFocus(); // New Terminal, active from the keyboard
    await userEvent.hover(items[3] as HTMLElement); // OpenCode
    expect(items[3]).toHaveFocus();
    expect(menu.querySelectorAll('[role="menuitem"]:focus')).toHaveLength(1);
    await userEvent.keyboard('{ArrowUp}'); // arrows go on from where the pointer is
    expect(items[2]).toHaveFocus();
    for (const item of items) expect(item.className).not.toMatch(/(^|\s)hover:bg-blue/);
  });

  it.each([
    ['New Terminal', 'terminal'],
    ['Claude Code', 'claude_code'],
    ['Codex', 'codex'],
    ['OpenCode', 'opencode'],
  ])('%s starts a %s session in the current project, without a worktree, and selects it', async (label, agent) => {
    await mount();
    const spy = create();
    await openMenu();
    await userEvent.click(screen.getByRole('menuitem', { name: new RegExp(`^${label}`) }));
    expect(spy).toHaveBeenCalledWith({ project_id: 'p1', agent, command: null, title: null, worktree: null, first_prompt: null });
    await waitFor(() => expect(getSelectedSessionId()).toBe('a8'));
    expect(screen.queryByRole('menu')).toBeNull();
  });

  it("uses the selected session's project", async () => {
    selectSession('a3');
    await mount();
    const spy = create();
    await openMenu();
    await userEvent.click(screen.getByRole('menuitem', { name: /^Codex/ }));
    expect(spy).toHaveBeenCalledWith(expect.objectContaining({ project_id: 'p2', agent: 'codex' }));
  });

  it('with no project at all it asks to add one instead of creating a session', async () => {
    for (const p of await client.listProjects()) await client.removeProject(p.id);
    selectSession(null);
    render(
      <ClientProvider client={client}>
        <TabStrip />
      </ClientProvider>,
    );
    await act(async () => {}); // let the snapshot (no projects) arrive
    const spy = create();
    const add = vi.fn();
    const off = onUiCommand('openAddProject', add);
    await openMenu();
    await userEvent.click(screen.getByRole('menuitem', { name: /^New Terminal/ }));
    expect(add).toHaveBeenCalledTimes(1);
    expect(spy).not.toHaveBeenCalled();
    off();
  });

  it('"Worktree or other project…" opens the sheet for the current project, "Agent settings…" opens Settings › Agents', async () => {
    await mount();
    const sheet = vi.fn();
    const off = onUiCommand('openNewSession', sheet);
    await openMenu();
    expect(screen.queryByRole('menuitem', { name: 'New session…' })).toBeNull(); // the old label read as a duplicate
    await userEvent.click(screen.getByRole('menuitem', { name: 'Worktree or other project…' }));
    expect(sheet).toHaveBeenCalledWith('p1');
    await openMenu();
    await userEvent.click(screen.getByRole('menuitem', { name: 'Agent settings…' }));
    expect(getOverlay()).toEqual({ name: 'settings', section: 'Agents' });
    off();
  });

  it('works from the keyboard: arrows skip separators, Enter starts, Esc closes', async () => {
    await mount();
    const spy = create();
    await openMenu();
    expect(screen.getByRole('menuitem', { name: /^New Terminal/ })).toHaveFocus();
    await userEvent.keyboard('{ArrowDown}');
    expect(screen.getByRole('menuitem', { name: /^Claude Code/ })).toHaveFocus();
    await userEvent.keyboard('{ArrowUp}{ArrowUp}'); // wraps to the last item
    expect(screen.getByRole('menuitem', { name: 'Agent settings…' })).toHaveFocus();
    await userEvent.keyboard('{Escape}');
    expect(screen.queryByRole('menu')).toBeNull();
    await openMenu();
    await userEvent.keyboard('{ArrowDown}{ArrowDown}{Enter}');
    expect(spy).toHaveBeenCalledWith(expect.objectContaining({ agent: 'codex' }));
  });
});

describe('drag to reorder', () => {
  it('dragging tab 3 to the front moves it, shows the insertion bar meanwhile and remembers the order', async () => {
    await addThird();
    await mount(3);
    layout();
    expect(tabNames()).toEqual(['Fix invoice rounding', 'Flyway migration V42', 'Third']);
    fireEvent.pointerDown(tab('Third'), { clientX: 250, button: 0 });
    expect(getSelectedSessionId()).toBe('a8'); // mouse down selects
    expect(screen.queryByTestId('tab-drop-indicator')).toBeNull();
    pointer('pointermove', 20);
    expect(screen.getByTestId('tab-drop-indicator')).toHaveStyle({ left: '-2px' });
    pointer('pointermove', 160);
    expect(screen.getByTestId('tab-drop-indicator')).toHaveStyle({ left: '202px' }); // past the last other tab
    pointer('pointermove', 20);
    pointer('pointerup', 20);
    expect(screen.queryByTestId('tab-drop-indicator')).toBeNull();
    expect(tabNames()).toEqual(['Third', 'Fix invoice rounding', 'Flyway migration V42']);
    expect(JSON.parse(localStorage.getItem('lotse-tab-order') ?? '{}')).toEqual({ p1: ['a8', 'a1', 'a2'] });
  });

  it('the saved order survives a reload; sessions created later join at the end', async () => {
    localStorage.setItem('lotse-tab-order', JSON.stringify({ p1: ['a2', 'a1'] }));
    await addThird();
    await mount(3);
    expect(tabNames()).toEqual(['Flyway migration V42', 'Fix invoice rounding', 'Third']);
  });

  it('dragging a tab down onto the panes reports the pointer and drops there instead of reordering', async () => {
    await addThird();
    await mount(3);
    layout();
    const seen: TabDrag[] = [];
    const off = onTabDrag((d) => {
      seen.push(d);
      return d.phase !== 'cancel';
    });
    const at = (type: 'pointermove' | 'pointerup', clientX: number, clientY: number) => fireEvent(window, new MouseEvent(type, { clientX, clientY }));
    fireEvent.pointerDown(tab('Third'), { clientX: 250, clientY: 16, button: 0 });
    at('pointermove', 250, 300); // straight down: no sideways movement needed to start the drag
    expect(seen.at(-1)).toEqual({ phase: 'move', sessionId: 'a8', x: 250, y: 300 });
    expect(screen.queryByTestId('tab-drop-indicator')).toBeNull(); // no reorder bar over the panes
    at('pointerup', 250, 300);
    // `restore`: what was selected before the mouse went down on the tab (mouse down selects), so the panes can undo that swap
    expect(seen.at(-1)).toEqual({ phase: 'drop', sessionId: 'a8', x: 250, y: 300, restore: 'a1' });
    expect(tabNames()).toEqual(['Fix invoice rounding', 'Flyway migration V42', 'Third']);
    expect(localStorage.getItem('lotse-tab-order')).toBeNull();
    off();
  });

  it('Esc while a tab hovers over the panes cancels the drop zone', async () => {
    await mount();
    layout();
    const seen: TabDrag[] = [];
    const off = onTabDrag((d) => (seen.push(d), d.phase === 'move'));
    fireEvent.pointerDown(tab('Flyway'), { clientX: 150, clientY: 16, button: 0 });
    fireEvent(window, new MouseEvent('pointermove', { clientX: 150, clientY: 300 }));
    fireEvent.keyDown(window, { key: 'Escape' });
    expect(seen.at(-1)).toEqual({ phase: 'cancel' });
    off();
  });

  it('a drag that never leaves the strip does not talk to the panes except to clear the zone', async () => {
    await mount();
    layout();
    const seen: TabDrag[] = [];
    const off = onTabDrag((d) => (seen.push(d), false));
    fireEvent.pointerDown(tab('Flyway'), { clientX: 150, clientY: 16, button: 0 });
    pointer('pointermove', 10);
    pointer('pointerup', 10);
    expect(seen.every((d) => d.phase !== 'drop')).toBe(true);
    off();
  });

  it('a small wobble while clicking does not reorder, and the click still selects', async () => {
    await mount();
    layout();
    fireEvent.pointerDown(tab('Flyway'), { clientX: 150, button: 0 });
    pointer('pointermove', 152);
    pointer('pointerup', 152);
    fireEvent.click(tab('Flyway'));
    expect(localStorage.getItem('lotse-tab-order')).toBeNull();
    expect(getSelectedSessionId()).toBe('a2');
  });

  it('Esc during a drag cancels it; the right button never starts one', async () => {
    await mount();
    layout();
    fireEvent.pointerDown(tab('Flyway'), { clientX: 150, button: 0 });
    pointer('pointermove', 10);
    expect(screen.getByTestId('tab-drop-indicator')).toBeInTheDocument();
    fireEvent.keyDown(window, { key: 'Escape' });
    expect(screen.queryByTestId('tab-drop-indicator')).toBeNull();
    expect(tabNames()).toEqual(['Fix invoice rounding', 'Flyway migration V42']);
    selectSession('a1');
    fireEvent.pointerDown(tab('Flyway'), { clientX: 150, button: 2 });
    expect(getSelectedSessionId()).toBe('a1');
  });

  it('a drag right after the last tab drops at the end', async () => {
    await addThird();
    await mount(3);
    layout();
    fireEvent.pointerDown(tab('Fix invoice'), { clientX: 50, button: 0 });
    pointer('pointermove', 290);
    pointer('pointerup', 290);
    expect(tabNames()).toEqual(['Flyway migration V42', 'Third', 'Fix invoice rounding']);
  });
});
