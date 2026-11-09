import { act, fireEvent, render, screen, waitFor, within } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { ClientProvider, MockClient } from '../lib/client';
import { emitUiCommand } from '../lib/ui/commands';
import { getSelectedSessionId, selectSession } from '../lib/ui/selection';
import { FakeTerminal } from '../terminal/fakeXterm';
import { layoutKey } from './layout';
import { Panes } from './Panes';

vi.mock('@xterm/xterm', async () => (await import('../terminal/fakeXterm')).xtermModule);
vi.mock('@xterm/addon-fit', async () => (await import('../terminal/fakeXterm')).fitModule);
vi.mock('@xterm/addon-web-links', async () => (await import('../terminal/fakeXterm')).linksModule);
vi.mock('@xterm/addon-webgl', async () => (await import('../terminal/fakeXterm')).webglModule);

/** The display the (faked) Tauri window is on; logical points. */
const monitor = vi.hoisted(() => ({ now: { name: 'Built-in Retina Display', width: 1512, height: 982 } }));
vi.mock('@tauri-apps/api/window', () => ({
  currentMonitor: async () => ({ name: monitor.now.name, scaleFactor: 2, size: { toLogical: () => ({ width: monitor.now.width, height: monitor.now.height }) } }),
}));
const LAPTOP = { name: 'Built-in Retina Display', width: 1512, height: 982 };
const WIDE = { name: 'Studio Display', width: 3440, height: 1440 };

// jsdom lacks layout rects; CodeMirror measures selections through Range.
Range.prototype.getClientRects = () => [] as unknown as DOMRectList;

let client: MockClient;
const mount = async () => {
  render(
    <ClientProvider client={client}>
      <Panes />
    </ClientProvider>,
  );
  await waitFor(() => expect(document.querySelector('[data-pane]')).not.toBeNull());
};
const paneIds = () => [...document.querySelectorAll('[data-pane]')].map((p) => p.getAttribute('data-pane'));
const pane = (id: string) => document.querySelector<HTMLElement>(`[data-pane="${id}"]`)!;
const bar = (id: string) => within(pane(id)).getByTestId('pane-status');
const rails = () => screen.queryAllByTestId('pane-rail');
const shown = (id: string) => pane(id).closest<HTMLElement>('[data-column]')!.style.display !== 'none';
const cmdD = () => fireEvent.keyDown(window, { code: 'KeyD', key: 'd', metaKey: true });
const useDisplay = async (d: typeof LAPTOP) => {
  monitor.now = d;
  await act(async () => {
    window.dispatchEvent(new Event('resize'));
  });
};

beforeEach(() => {
  FakeTerminal.reset();
  client = new MockClient({ script: false });
  selectSession('a1');
  monitor.now = LAPTOP;
});
afterEach(() => {
  client.close();
  selectSession(null);
  delete (window as unknown as Record<string, unknown>).__TAURI_INTERNALS__;
});

describe('pane status bars', () => {
  it('each_pane_has_status_bar', async () => {
    await mount();
    const { projects } = await client.getSnapshot();
    const project = projects.find((p) => p.id === 'p1')!;
    // a single pane has one too: agent, title · project, status, branch ↑↓±
    expect(screen.getAllByTestId('pane-status')).toHaveLength(1);
    const one = bar('a1');
    expect(one).toHaveTextContent('Fix invoice rounding');
    expect(one).toHaveTextContent(project.name);
    expect(within(one).getByRole('img', { name: 'In progress' })).toBeInTheDocument();
    expect(one).toHaveTextContent('fix-rounding');
    expect(one).toHaveTextContent('↑2');
    expect(one).toHaveTextContent('↓1');
    expect(one).toHaveTextContent('±3');
    expect(within(one).getByLabelText('Claude Code')).toBeInTheDocument();
    expect(within(one).queryByRole('button', { name: 'Close pane' })).toBeNull(); // the last pane stays open

    cmdD();
    cmdD();
    expect(screen.getAllByTestId('pane-status')).toHaveLength(3);
    for (const id of paneIds()) expect(bar(id!)).toHaveTextContent(/·/);
    expect(screen.getAllByRole('button', { name: 'Close pane' })).toHaveLength(3);
  });

  it('a session without git shows no branch', async () => {
    selectSession('a7');
    await mount();
    expect(bar('a7')).toHaveTextContent('Scratch shell');
    expect(bar('a7')).not.toHaveTextContent('↑');
  });

  it('editor panes carry the bar too: language and Saved', async () => {
    await mount();
    act(() => void emitUiCommand('openFile', 'a1', 'src/invoice/totals.ts'));
    const key = paneIds()[1]!;
    expect(within(pane(key)).getByTestId('pane-status')).toHaveTextContent('totals.ts');
    await within(pane(key)).findByText('Saved');
    expect(within(pane(key)).getByText(/TypeScript/)).toBeInTheDocument();
  });

  it('focused_pane_marked', async () => {
    await mount();
    cmdD();
    const [first, second] = paneIds() as string[];
    expect(bar(second!)).toHaveAttribute('data-focused', 'true');
    expect(bar(first!)).toHaveAttribute('data-focused', 'false');
    fireEvent.mouseDown(pane(first!));
    expect(bar(first!)).toHaveAttribute('data-focused', 'true');
    expect(bar(second!)).toHaveAttribute('data-focused', 'false');
  });
});

describe('rail and hide', () => {
  it('collapses a column to a rail, keeps its terminal, and restores on click', async () => {
    await mount();
    cmdD();
    const [first, second] = paneIds() as string[];
    expect(rails()).toHaveLength(0);
    fireEvent.click(within(pane(second!)).getByRole('button', { name: 'Collapse to rail' }));
    expect(rails()).toHaveLength(1);
    expect(paneIds()).toEqual([first, second]); // still mounted, so its scrollback survives
    expect(shown(second!)).toBe(false);
    expect(getSelectedSessionId()).toBe(first);
    expect(within(rails()[0]!).getByRole('img')).toBeInTheDocument(); // status of the session

    fireEvent.click(screen.getByRole('button', { name: /^Restore / }));
    expect(rails()).toHaveLength(0);
    expect(shown(second!)).toBe(true);
    expect(getSelectedSessionId()).toBe(second);
    expect(pane(second!)).toHaveAttribute('data-focused', 'true');
  });

  it('hides a column and brings it back when its session is selected', async () => {
    await mount();
    cmdD();
    const [first, second] = paneIds() as string[];
    fireEvent.click(within(pane(second!)).getByRole('button', { name: 'Hide column' }));
    expect(rails()).toHaveLength(0);
    expect(shown(second!)).toBe(false);
    expect(getSelectedSessionId()).toBe(first);
    act(() => selectSession(second!)); // a sidebar card or a tab
    expect(shown(second!)).toBe(true);
    expect(pane(second!)).toHaveAttribute('data-focused', 'true');
  });

  it('offers neither with a single column', async () => {
    await mount();
    expect(screen.queryByRole('button', { name: 'Collapse to rail' })).toBeNull();
    expect(screen.queryByRole('button', { name: 'Hide column' })).toBeNull();
  });
});

describe('displays', () => {
  beforeEach(() => {
    (window as unknown as Record<string, unknown>).__TAURI_INTERNALS__ = {};
  });

  it('allows six panes on a wide display, four on a laptop', async () => {
    await mount();
    for (let i = 0; i < 5; i++) cmdD();
    expect(paneIds()).toHaveLength(4);
    await useDisplay(WIDE);
    for (let i = 0; i < 5; i++) cmdD();
    expect(paneIds()).toHaveLength(6);
  });

  it('a display restores its own layout', async () => {
    await mount();
    cmdD();
    const laptop = paneIds();
    expect(laptop).toHaveLength(2);
    await waitFor(() => expect(JSON.parse(localStorage.getItem(layoutKey(LAPTOP)) ?? '{}').panes).toEqual(laptop));

    await useDisplay(WIDE);
    for (let i = 0; i < 3; i++) cmdD();
    const wide = paneIds();
    expect(wide).toHaveLength(5);
    await waitFor(() => expect(JSON.parse(localStorage.getItem(layoutKey(WIDE)) ?? '{}').panes).toEqual(wide));

    await useDisplay(LAPTOP);
    await waitFor(() => expect(paneIds()).toEqual(laptop));
    await useDisplay(WIDE);
    await waitFor(() => expect(paneIds()).toEqual(wide));
  });

  it('starts from the layout saved for this display', async () => {
    localStorage.setItem(layoutKey(LAPTOP), JSON.stringify({ panes: ['a3', 'a1', 'a2'], focus: 1, weights: {} }));
    selectSession('a1');
    await mount();
    await waitFor(() => expect(paneIds()).toEqual(['a3', 'a1', 'a2']));
    expect(pane('a1')).toHaveAttribute('data-focused', 'true');
  });
});
