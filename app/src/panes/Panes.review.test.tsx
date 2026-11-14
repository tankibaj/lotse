import { EditorView } from '@codemirror/view';
import { act, fireEvent, render, screen, waitFor, within } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { ClientProvider, MockClient } from '../lib/client';
import type { Project, Session } from '../lib/client';
import { emitUiCommand } from '../lib/ui/commands';
import { getSelectedSessionId, selectSession } from '../lib/ui/selection';
import { FakeTerminal } from '../terminal/fakeXterm';
import { layoutKey, paneKey } from './layout';
import { Panes, Rail } from './Panes';

vi.mock('@xterm/xterm', async () => (await import('../terminal/fakeXterm')).xtermModule);
vi.mock('@xterm/addon-fit', async () => (await import('../terminal/fakeXterm')).fitModule);
vi.mock('@xterm/addon-web-links', async () => (await import('../terminal/fakeXterm')).linksModule);
vi.mock('@xterm/addon-webgl', async () => (await import('../terminal/fakeXterm')).webglModule);

const monitor = vi.hoisted(() => ({ now: { name: 'Built-in Retina Display', width: 1512, height: 982 }, lookups: 0 }));
vi.mock('@tauri-apps/api/window', () => ({
  currentMonitor: async () => {
    monitor.lookups += 1;
    return {
      name: monitor.now.name,
      scaleFactor: 2,
      size: { toLogical: () => ({ width: monitor.now.width, height: monitor.now.height }) },
      position: { toLogical: () => ({ x: 0, y: 0 }) },
    };
  },
}));
const LAPTOP = { name: 'Built-in Retina Display', width: 1512, height: 982, scale: 2 };
const WIDE = { name: 'Studio Display', width: 3440, height: 1440, scale: 2 };
const OTHER = { name: 'Projector', width: 1600, height: 900, scale: 2 };
const TOTALS = 'src/invoice/totals.ts';
const edKey = (sessionId: string, path: string) => paneKey({ kind: 'editor', sessionId, path });

Range.prototype.getClientRects = () => [] as unknown as DOMRectList;

let client: MockClient;
const mount = async () => {
  render(
    <ClientProvider client={client}>
      <Panes />
    </ClientProvider>,
  );
  await waitFor(() => expect(document.querySelector('[data-pane]')).not.toBeNull());
  await waitFor(() => expect(monitor.lookups).toBeGreaterThan(0));
  await act(async () => {});
};
const paneIds = () => [...document.querySelectorAll('[data-pane]')].map((p) => p.getAttribute('data-pane'));
const pane = (id: string) => document.querySelector<HTMLElement>(`[data-pane="${id}"]`)!;
const shown = (id: string) => pane(id).closest<HTMLElement>('[data-column]')!.style.display !== 'none';
const cmd = (code: string, key: string, init: KeyboardEventInit = {}) => fireEvent.keyDown(window, { code, key, metaKey: true, ...init });
const cmdD = () => cmd('KeyD', 'd');
const openFile = (sessionId: string, path: string) => act(() => void emitUiCommand('openFile', sessionId, path));
const editorView = () => EditorView.findFromDOM(document.querySelector<HTMLElement>('.cm-editor')!)!;
const makeDirty = async () => {
  await waitFor(() => expect(editorView().state.doc.length).toBeGreaterThan(0));
  act(() => editorView().dispatch({ changes: { from: 0, insert: 'x' } }));
};
/** Move the window to `d` and wait until the (debounced) lookup has run. */
const moveTo = async (d: { name: string; width: number; height: number }) => {
  monitor.now = d;
  const before = monitor.lookups;
  window.dispatchEvent(new Event('resize'));
  await waitFor(() => expect(monitor.lookups).toBeGreaterThan(before));
  await act(async () => {});
};

beforeEach(() => {
  FakeTerminal.reset();
  client = new MockClient({ script: false });
  selectSession('a1');
  monitor.now = LAPTOP;
  monitor.lookups = 0;
  (window as unknown as Record<string, unknown>).__TAURI_INTERNALS__ = {};
});
afterEach(() => {
  client.close();
  selectSession(null);
  delete (window as unknown as Record<string, unknown>).__TAURI_INTERNALS__;
  vi.restoreAllMocks();
});

describe('review 1: focus reopens a column', () => {
  it('splitting an open session that sits in a hidden column shows it again and focuses it', async () => {
    await mount();
    cmdD();
    const [, second] = paneIds() as string[];
    fireEvent.click(within(pane(second!)).getByRole('button', { name: 'Hide column' }));
    expect(shown(second!)).toBe(false);
    act(() => void emitUiCommand('splitSession', second));
    expect(shown(second!)).toBe(true);
    expect(pane(second!)).toHaveAttribute('data-focused', 'true');
  });
});

describe('review 2, 3, 5: restoring a display', () => {
  it('keeps an editor with unsaved edits when the display restores a stored layout', async () => {
    localStorage.setItem(layoutKey(WIDE), JSON.stringify({ panes: ['a1', 'a2', 'a3'], focus: 0, weights: {} }));
    await mount();
    openFile('a1', TOTALS);
    await waitFor(() => expect(document.querySelector('.cm-content')).not.toBeNull());
    await makeDirty();
    const key = edKey('a1', TOTALS);
    await moveTo(WIDE);
    expect(paneIds()).toEqual(['a1', 'a2', 'a3', key]);
    expect(within(pane(key)).getByTitle('Unsaved changes')).toBeInTheDocument();
  });

  it('a closed editor does not leave a stale unsaved mark behind', async () => {
    await mount();
    openFile('a1', TOTALS);
    await waitFor(() => expect(document.querySelector('.cm-content')).not.toBeNull());
    await makeDirty();
    cmd('KeyW', 'w');
    cmd('KeyW', 'w'); // second ⌘W discards
    expect(paneIds()).toEqual(['a1']);
    openFile('a1', TOTALS);
    await waitFor(() => expect(document.querySelector('.cm-content')).not.toBeNull());
    expect(within(pane(edKey('a1', TOTALS))).queryByTitle('Unsaved changes')).toBeNull();
    cmd('KeyW', 'w');
    expect(paneIds()).toEqual(['a1']); // closed at once, no discard prompt
  });

  it('drops sessions that no longer exist from a stored layout and selects the focused one that does', async () => {
    localStorage.setItem(layoutKey(WIDE), JSON.stringify({ panes: ['a1', 'gone', 'a2'], focus: 1, weights: {} }));
    await mount();
    await moveTo(WIDE);
    expect(paneIds()).toEqual(['a1', 'a2']);
    expect(getSelectedSessionId()).toBe('a2');
    expect(pane('a2')).toHaveAttribute('data-focused', 'true');
  });

  it('trims the layout to the new display when nothing is stored for it', async () => {
    await mount();
    await moveTo(WIDE);
    for (let i = 0; i < 5; i++) cmdD();
    expect(paneIds()).toHaveLength(6);
    await moveTo(OTHER);
    expect(paneIds()).toHaveLength(4);
  });
});

describe('review 6: debouncing', () => {
  it('looks the monitor up once for a burst of resize events', async () => {
    await mount();
    const before = monitor.lookups;
    for (let i = 0; i < 5; i++) window.dispatchEvent(new Event('resize'));
    expect(monitor.lookups).toBe(before);
    await waitFor(() => expect(monitor.lookups).toBe(before + 1));
  });

  it('writes the layout once for a burst of changes', async () => {
    await mount();
    cmdD();
    cmdD();
    await waitFor(() => expect(localStorage.getItem(layoutKey(LAPTOP))).not.toBeNull());
    const set = vi.spyOn(Storage.prototype, 'setItem');
    for (let i = 0; i < 5; i++) cmd('BracketLeft', '[');
    expect(set).not.toHaveBeenCalled();
    await waitFor(() => expect(set).toHaveBeenCalledTimes(1));
    await act(async () => {});
    expect(set).toHaveBeenCalledTimes(1);
  });
});

describe('review 7: a rail never hides unsaved work', () => {
  /** a1 | (editor over a2): the editor is dirty. */
  const stackWithEditor = async (dirty: boolean) => {
    await mount();
    openFile('a1', TOTALS);
    await waitFor(() => expect(document.querySelector('.cm-content')).not.toBeNull());
    if (dirty) await makeDirty();
    cmd('KeyD', 'd', { shiftKey: true }); // a2 below the focused editor
    expect(paneIds()).toEqual(['a1', edKey('a1', TOTALS), expect.any(String)]);
  };

  it.each(['Collapse to rail', 'Hide column'])('"%s" is refused while a pane of the column has unsaved edits', async (name) => {
    await stackWithEditor(true);
    const below = paneIds()[2]!;
    fireEvent.click(within(pane(below)).getByRole('button', { name }));
    expect(screen.queryAllByTestId('pane-rail')).toHaveLength(0);
    expect(shown(below)).toBe(true);
    expect(screen.getByRole('status')).toHaveTextContent(/Unsaved changes in totals\.ts/);
  });

  it('a saved editor may go to the rail, which names it', async () => {
    await stackWithEditor(false);
    const below = paneIds()[2]!;
    fireEvent.click(within(pane(below)).getByRole('button', { name: 'Collapse to rail' }));
    expect(screen.getByTestId('pane-rail')).toHaveAccessibleName(/totals\.ts/);
  });

  it('the rail of an editor-only column is labelled with the file', () => {
    const editor = { kind: 'editor' as const, sessionId: 'a1', path: TOTALS };
    render(<Rail column={[editor]} projects={[] as Project[]} sessions={[] as Session[]} onRestore={() => {}} />);
    expect(screen.getByTestId('pane-rail')).toHaveAccessibleName('Restore totals.ts');
  });
});

describe('palette pane commands', () => {
  it('focusPane steps the focus and closePane closes the focused pane', async () => {
    await mount();
    cmdD();
    const [first, second] = paneIds() as string[];
    expect(pane(second!)).toHaveAttribute('data-focused', 'true');
    act(() => void emitUiCommand('focusPane', 'prev'));
    expect(pane(first!)).toHaveAttribute('data-focused', 'true');
    act(() => void emitUiCommand('closePane'));
    expect(paneIds()).toEqual([second]);
  });
});

describe('review: saving on quit', () => {
  it('flushes a pending layout save when the page is hidden before the debounce fires', async () => {
    await mount();
    cmdD();
    window.dispatchEvent(new Event('pagehide'));
    const keys = Object.keys(localStorage).filter((k) => k.startsWith('lotse-panes:'));
    const saved = keys.map((k) => localStorage.getItem(k) ?? '').join('');
    expect(saved.length).toBeGreaterThan(0);
    expect(JSON.parse(localStorage.getItem(keys[0]!)!).panes).toHaveLength(2);
  });
});
