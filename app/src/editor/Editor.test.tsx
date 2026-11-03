import { EditorView } from '@codemirror/view';
import { act, fireEvent, render, screen, waitFor } from '@testing-library/react';
import { afterEach, beforeAll, beforeEach, describe, expect, it, vi } from 'vitest';
import { ClientProvider, MockClient } from '../lib/client';
import { Editor } from './Editor';

const TOTALS = 'src/invoice/totals.ts';
const CHANGE = { added: 2, removed: 1, by: 'claude in “Fix totals”' };

let client: MockClient;

beforeAll(() => {
  // jsdom has no layout; CodeMirror measures through Range.
  const rects = { length: 0, item: () => null, [Symbol.iterator]: [][Symbol.iterator] } as unknown as DOMRectList;
  Range.prototype.getClientRects = () => rects;
  Range.prototype.getBoundingClientRect = () => new DOMRect();
});
beforeEach(() => {
  client = new MockClient({ script: false });
});
afterEach(() => client.close());

const mount = (props: Partial<Parameters<typeof Editor>[0]> = {}) =>
  render(
    <ClientProvider client={client}>
      <Editor sessionId="a1" path={TOTALS} {...props} />
    </ClientProvider>,
  );
const view = () => EditorView.findFromDOM(document.querySelector<HTMLElement>('.cm-editor')!)!;
const loaded = () => waitFor(() => expect(document.querySelector('.cm-editor') && view().state.doc.length).toBeGreaterThan(0));
const typeAtStart = (insert: string) => act(() => view().dispatch({ changes: { from: 0, insert } }));
const cmdS = () => fireEvent.keyDown(view().contentDOM, { key: 's', code: 'KeyS', keyCode: 83, metaKey: true });

describe('Editor', () => {
  it('saves_with_cmd_s', async () => {
    const write = vi.spyOn(client, 'writeFile');
    mount();
    await loaded();
    const before = await client.readFile('a1', TOTALS);
    expect(screen.getByText('Saved')).toBeInTheDocument();
    typeAtStart('// hi\n');
    expect(screen.getByText('Edited · ⌘S to save')).toBeInTheDocument();
    cmdS();
    await waitFor(() => expect(write).toHaveBeenCalledWith('a1', TOTALS, `// hi\n${before.text}`, before.sha));
    await screen.findByText('Saved');
    expect((await client.readFile('a1', TOTALS)).text).toBe(`// hi\n${before.text}`);
  });

  it('conflict_shows_reload', async () => {
    mount();
    await loaded();
    await client.writeFile('a1', TOTALS, 'changed elsewhere\n'); // another writer moves the sha
    typeAtStart('// mine\n');
    cmdS();
    await screen.findByText(/Changed on disk/);
    fireEvent.click(screen.getByRole('button', { name: 'Reload' }));
    await waitFor(() => expect(view().state.doc.toString()).toBe('changed elsewhere\n'));
    expect(screen.getByText('Saved')).toBeInTheDocument();
    // the new sha is the base again: the next save goes through
    typeAtStart('x');
    cmdS();
    await waitFor(async () => expect((await client.readFile('a1', TOTALS)).text).toBe('xchanged elsewhere\n'));
  });

  it('diff_toggle_marks_added_lines', async () => {
    mount({ change: CHANGE });
    await loaded();
    expect(screen.getByText(/Changed by claude in “Fix totals”/)).toBeInTheDocument();
    expect(document.querySelectorAll('.cm-changedLine')).toHaveLength(0);
    const toggle = screen.getByRole('button', { name: 'Diff' });
    expect(toggle).toHaveAttribute('aria-pressed', 'false');
    fireEvent.click(toggle);
    await waitFor(() => expect(document.querySelectorAll('.cm-changedLine').length).toBeGreaterThan(0));
    const added = Array.from(document.querySelectorAll('.cm-changedLine')).map((l) => l.textContent);
    expect(added.some((t) => t?.includes('return roundHalfEven(sum, 2)'))).toBe(true);
    expect(document.querySelector('.cm-deletedChunk')?.textContent).toContain('Math.round(sum * 100) / 100');
    expect(toggle).toHaveAttribute('aria-pressed', 'true');
    expect(screen.getByText('Read-only while diff is shown')).toBeInTheDocument();
    expect(view().state.readOnly).toBe(true);
    fireEvent.click(toggle);
    await waitFor(() => expect(document.querySelectorAll('.cm-changedLine')).toHaveLength(0));
    expect(view().state.readOnly).toBe(false);
  });

  it('binary_file_read_only', async () => {
    const write = vi.spyOn(client, 'writeFile');
    vi.spyOn(client, 'readFile').mockResolvedValue({ path: 'docs/logo.png', text: '', sha: 'abc', binary: true, size: 2048 });
    mount({ path: 'docs/logo.png' });
    await screen.findByText(/Binary file/);
    expect(view().state.readOnly).toBe(true);
    expect(view().contentDOM).toHaveAttribute('contenteditable', 'false');
    cmdS();
    expect(write).not.toHaveBeenCalled();
  });

  it('opens_at_first_change', async () => {
    mount({ change: CHANGE });
    await loaded();
    await waitFor(() => expect(view().state.doc.lineAt(view().state.selection.main.head).text).toContain('return roundHalfEven(sum, 2)'));
  });

  it('saves_crlf_file_with_crlf', async () => {
    vi.spyOn(client, 'readFile').mockResolvedValue({ path: 'a.txt', text: 'a\r\nb\r\n', sha: 's', binary: false, size: 6 });
    const write = vi.spyOn(client, 'writeFile').mockResolvedValue({ path: 'a.txt', text: 'xa\r\nb\r\n', sha: 't', binary: false, size: 7 });
    mount({ path: 'a.txt' });
    await loaded();
    expect(screen.getByText('UTF-8 · CRLF')).toBeInTheDocument();
    typeAtStart('x');
    cmdS();
    await waitFor(() => expect(write).toHaveBeenCalledWith('a1', 'a.txt', 'xa\r\nb\r\n', 's'));
  });

  it('shows the language, indent and a load error', async () => {
    mount();
    await loaded();
    expect(screen.getByText('TypeScript')).toBeInTheDocument();
    expect(screen.getByText('Spaces: 2')).toBeInTheDocument();
    expect(screen.getByText('UTF-8 · LF')).toBeInTheDocument();
  });

  it('a file that cannot be read shows the error and no editor', async () => {
    mount({ path: 'nope.ts' });
    expect(await screen.findByRole('alert')).toHaveTextContent('no file nope.ts');
  });
});
