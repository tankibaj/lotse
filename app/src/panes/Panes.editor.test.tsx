import { act, fireEvent, render, screen, waitFor, within } from '@testing-library/react';
import { EditorView } from '@codemirror/view';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { ClientProvider, MockClient } from '../lib/client';
import { emitUiCommand } from '../lib/ui/commands';
import { getSelectedSessionId, selectSession } from '../lib/ui/selection';
import { FakeTerminal } from '../terminal/fakeXterm';
import { paneKey } from './layout';
import { Panes } from './Panes';

vi.mock('@xterm/xterm', async () => (await import('../terminal/fakeXterm')).xtermModule);
vi.mock('@xterm/addon-fit', async () => (await import('../terminal/fakeXterm')).fitModule);
vi.mock('@xterm/addon-web-links', async () => (await import('../terminal/fakeXterm')).linksModule);
vi.mock('@xterm/addon-webgl', async () => (await import('../terminal/fakeXterm')).webglModule);

const TOTALS = 'src/invoice/totals.ts';
const edKey = (sessionId: string, path: string) => paneKey({ kind: 'editor', sessionId, path });

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
const openFile = (sessionId: string, path: string) => act(() => void emitUiCommand('openFile', sessionId, path));
const editorView = () => EditorView.findFromDOM(document.querySelector<HTMLElement>('.cm-editor')!)!;
const cmContent = () => document.querySelector<HTMLElement>('.cm-content')!;
const mod = (target: Element | Window, k: string, init: KeyboardEventInit = {}) => fireEvent.keyDown(target, { key: k, metaKey: true, ...init });

beforeEach(() => {
  FakeTerminal.reset();
  client = new MockClient({ script: false });
  selectSession('a1');
});
afterEach(() => {
  client.close();
  selectSession(null);
});

describe('editor panes', () => {
  it('open_file_adds_editor_beside_terminal', async () => {
    await mount();
    openFile('a1', TOTALS);
    expect(paneIds()).toEqual(['a1', edKey('a1', TOTALS)]);
    const ed = pane(edKey('a1', TOTALS));
    expect(ed).toHaveAttribute('data-focused', 'true');
    expect(within(ed).getByText('totals.ts')).toBeInTheDocument();
    expect(within(ed).getByTitle(/\/src\/invoice\/totals\.ts$/)).toHaveTextContent('src/invoice');
    await waitFor(() => expect(ed.querySelector('.cm-content')).not.toBeNull());
    expect(getSelectedSessionId()).toBe('a1');
  });

  it('the terminal keeps its place and the editor opens after it, even with other panes open', async () => {
    await mount();
    act(() => void emitUiCommand('splitSession', 'a4'));
    openFile('a1', TOTALS);
    expect(paneIds()).toEqual(['a1', edKey('a1', TOTALS), 'a4']);
  });

  it('asking for an open file focuses it; a session that is not open gets its terminal first', async () => {
    await mount();
    openFile('a1', TOTALS);
    act(() => void emitUiCommand('splitSession', 'a4'));
    openFile('a1', TOTALS);
    expect(paneIds()).toHaveLength(3);
    expect(pane(edKey('a1', TOTALS))).toHaveAttribute('data-focused', 'true');

    openFile('a3', 'README.md');
    expect(paneIds()).toContain('a3');
    expect(paneIds()).toContain(edKey('a3', 'README.md'));
  });

  it('when full it replaces the last editor and says so when only terminals are left', async () => {
    await mount();
    openFile('a1', TOTALS);
    act(() => void emitUiCommand('splitSession', 'a4'));
    act(() => void emitUiCommand('splitSession', 'a3'));
    expect(paneIds()).toHaveLength(4);
    openFile('a1', 'package.json');
    expect(paneIds()).toEqual(['a1', edKey('a1', 'package.json'), 'a4', 'a3']);

    // only terminals: nothing to replace
    fireEvent.click(within(pane(edKey('a1', 'package.json'))).getByRole('button', { name: 'Close editor' }));
    act(() => void emitUiCommand('splitSession', 'a2'));
    expect(paneIds()).toHaveLength(4);
    openFile('a1', 'README.md');
    expect(paneIds()).toHaveLength(4);
    expect(screen.getByRole('status')).toHaveTextContent('Up to 4 panes side by side');
  });

  it('close: the ✕ and ⌘W close the focused editor and focus returns to the terminal', async () => {
    await mount();
    openFile('a1', TOTALS);
    await waitFor(() => expect(cmContent()).not.toBeNull());
    mod(cmContent(), 'w');
    expect(paneIds()).toEqual(['a1']);
    expect(pane('a1')).toHaveAttribute('data-focused', 'true');

    openFile('a1', 'package.json');
    fireEvent.click(within(pane(edKey('a1', 'package.json'))).getByRole('button', { name: 'Close editor' }));
    expect(paneIds()).toEqual(['a1']);
  });

  it('⌘[ and ⌘] move focus between terminal and editor from inside the editor without indenting', async () => {
    await mount();
    openFile('a1', TOTALS);
    await waitFor(() => expect(cmContent()).not.toBeNull());
    const before = editorView().state.doc.toString();
    mod(cmContent(), '[');
    expect(pane('a1')).toHaveAttribute('data-focused', 'true');
    mod(window, ']');
    expect(pane(edKey('a1', TOTALS))).toHaveAttribute('data-focused', 'true');
    mod(cmContent(), ']');
    expect(pane('a1')).toHaveAttribute('data-focused', 'true');
    expect(editorView().state.doc.toString()).toBe(before);
  });

  it('shows an unsaved dot after an edit and asks before ⌘W throws the edit away', async () => {
    await mount();
    openFile('a1', TOTALS);
    await waitFor(() => expect(cmContent()).not.toBeNull());
    const ed = pane(edKey('a1', TOTALS));
    expect(within(ed).queryByTitle('Unsaved changes')).toBeNull();
    await waitFor(() => expect(editorView().state.doc.length).toBeGreaterThan(0));
    act(() => editorView().dispatch({ changes: { from: 0, insert: 'x' } }));
    expect(within(ed).getByTitle('Unsaved changes')).toBeInTheDocument();

    mod(cmContent(), 'w');
    expect(paneIds()).toHaveLength(2);
    expect(screen.getByRole('status')).toHaveTextContent(/Unsaved changes/);
    mod(cmContent(), 'w');
    expect(paneIds()).toEqual(['a1']);
  });

  it('shows what the agent changed in the file when the session has a diff for it', async () => {
    vi.spyOn(client, 'getDiff').mockResolvedValue({
      is_git: true,
      truncated: false,
      files: [{ path: TOTALS, old_path: null, status: 'modified', binary: false, truncated: false, diff: '', added: 7, removed: 2 }],
    });
    await mount();
    openFile('a1', TOTALS);
    expect(await screen.findByText(/Changed by/)).toBeInTheDocument();
    expect(screen.getByText('+7')).toBeInTheDocument();
    expect(screen.getByText('−2')).toBeInTheDocument();
  });

  it('Review in the change bar opens the full review on that file', async () => {
    vi.spyOn(client, 'getDiff').mockResolvedValue({
      is_git: true,
      truncated: false,
      files: [{ path: TOTALS, old_path: null, status: 'modified', binary: false, truncated: false, diff: '', added: 1, removed: 0 }],
    });
    const review = vi.fn();
    const { onUiCommand } = await import('../lib/ui/commands');
    const off = onUiCommand('openReview', review);
    await mount();
    openFile('a1', TOTALS);
    fireEvent.click(await screen.findByRole('button', { name: /Review/ }));
    expect(review).toHaveBeenCalledWith('a1', TOTALS);
    off();
  });
});

describe('expand', () => {
  const visible = (id: string) => pane(id).style.display !== 'none' && pane(id).closest<HTMLElement>('[data-column]')!.style.display !== 'none';

  it('expand_and_restore', async () => {
    await mount();
    act(() => void emitUiCommand('splitSession', 'a4'));
    openFile('a1', TOTALS);
    const key = edKey('a1', TOTALS);
    await waitFor(() => expect(cmContent()).not.toBeNull());

    fireEvent.click(within(pane(key)).getByRole('button', { name: 'Expand editor' }));
    expect(visible(key)).toBe(true);
    expect(visible('a1')).toBe(false);
    expect(visible('a4')).toBe(false);
    expect(screen.queryAllByTestId('pane-divider').every((d) => d.style.display === 'none')).toBe(true);

    fireEvent.keyDown(cmContent(), { key: 'Escape' });
    expect(visible('a1')).toBe(true);
    expect(visible('a4')).toBe(true);

    mod(cmContent(), 'Enter', { altKey: true });
    expect(visible('a1')).toBe(false);
    expect(within(pane(key)).getByRole('button', { name: 'Restore editor' })).toBeInTheDocument();
    mod(cmContent(), 'Enter', { altKey: true });
    expect(visible('a1')).toBe(true);
  });

  it('closing the expanded editor or opening another pane restores the layout', async () => {
    await mount();
    openFile('a1', TOTALS);
    const key = edKey('a1', TOTALS);
    await waitFor(() => expect(cmContent()).not.toBeNull());
    mod(cmContent(), 'Enter', { altKey: true });
    expect(visible('a1')).toBe(false);
    openFile('a1', 'package.json');
    expect(visible('a1')).toBe(true);
    mod(cmContent(), 'Enter', { altKey: true });
    fireEvent.click(within(pane(edKey('a1', 'package.json'))).getByRole('button', { name: 'Close editor' }));
    expect(visible('a1')).toBe(true);
    expect(pane(key)).toBeTruthy();
  });

  it('Esc elsewhere is left alone (a terminal needs it)', async () => {
    await mount();
    openFile('a1', TOTALS);
    await waitFor(() => expect(cmContent()).not.toBeNull());
    const esc = fireEvent.keyDown(document.querySelector('[data-pane="a1"]')!, { key: 'Escape' });
    expect(esc).toBe(true);
  });
});
