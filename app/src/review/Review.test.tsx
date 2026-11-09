import { EditorView } from '@codemirror/view';
import { act, cleanup, configure, fireEvent, render, screen, waitFor, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { afterEach, beforeAll, beforeEach, describe, expect, it, vi } from 'vitest';
import { ClientProvider, MockClient } from '../lib/client';
import type { DiffFile, GitDiff } from '../lib/client/types';
import { emitUiCommand } from '../lib/ui/commands';
import { closeReview } from '../lib/ui/panel';
import { selectSession } from '../lib/ui/selection';
import { Shell } from '../shell/Shell';
import { clearComments, getComments } from './comments';
import { Review } from './index';

// The Shell test does the same: the drop watcher talks to the webview only inside Tauri.
vi.mock('@tauri-apps/api/webview', () => ({ getCurrentWebview: () => ({ onDragDropEvent: async () => () => {} }) }));

const TOTALS = 'src/invoice/totals.ts';
const ROUND = 'src/invoice/round.ts';
const SPEC = 'test/totals.spec.ts';

const TOTALS_PATCH = `diff --git a/${TOTALS} b/${TOTALS}\n--- a/${TOTALS}\n+++ b/${TOTALS}\n@@ -41,2 +41,3 @@\n-  return Math.round(sum * 100) / 100\n+  return roundHalfEven(sum, 2)\n+  // banker's rounding avoids 0.005 drift\n }\n`;
const ROUND_PATCH = `diff --git a/${ROUND} b/${ROUND}\nnew file mode 100644\n--- /dev/null\n+++ b/${ROUND}\n@@ -0,0 +1,7 @@\n+export function roundHalfEven() {}\n`;
const SPEC_PATCH = `diff --git a/${SPEC} b/${SPEC}\n--- a/${SPEC}\n+++ b/${SPEC}\n@@ -14,1 +14,1 @@\n-    expect(invoiceTotal(lines)).toBe(10.01)\n+    expect(invoiceTotal(lines)).toBe(10.0)\n`;

const file = (path: string, over: Partial<DiffFile> = {}): DiffFile => ({
  path,
  old_path: null,
  status: 'modified',
  binary: false,
  truncated: false,
  diff: '',
  ...over,
});
const FILES: DiffFile[] = [
  file(TOTALS, { diff: TOTALS_PATCH, added: 2, removed: 1 }),
  file(ROUND, { status: 'added', diff: ROUND_PATCH, added: 7, removed: 0 }),
  file(SPEC, { diff: SPEC_PATCH, added: 1, removed: 1 }),
];

let client: MockClient;
const writeText = vi.fn().mockResolvedValue(undefined);

beforeAll(() => {
  configure({ getElementError: (message) => new Error(message ?? '') }); // no DOM dump: the review is a big page
  // jsdom has no layout; CodeMirror measures through Range.
  const rects = { length: 0, item: () => null, [Symbol.iterator]: [][Symbol.iterator] } as unknown as DOMRectList;
  Range.prototype.getClientRects = () => rects;
  Range.prototype.getBoundingClientRect = () => new DOMRect();
});
beforeEach(() => {
  client = new MockClient({ script: false });
  vi.spyOn(client, 'getDiff').mockResolvedValue({ is_git: true, files: FILES, truncated: false });
  writeText.mockClear();
  Object.defineProperty(navigator, 'clipboard', { value: { writeText }, configurable: true });
});
afterEach(() => {
  cleanup();
  clearComments('a1');
  client.close();
});

const onClose = vi.fn();
const ui = (props: Partial<Parameters<typeof Review>[0]> = {}) => (
  <ClientProvider client={client}>
    <Review sessionId="a1" onClose={onClose} {...props} />
  </ClientProvider>
);
const mount = (props: Partial<Parameters<typeof Review>[0]> = {}) => render(ui(props));
/** The diff of every file is on screen. */
const loaded = (n = FILES.length) => waitFor(() => expect(document.querySelectorAll('[data-file] .cm-editor').length).toBeGreaterThanOrEqual(n));
const sections = () => [...document.querySelectorAll('[data-file]')].map((s) => s.getAttribute('data-file'));
const sidebar = () => within(screen.getByRole('complementary', { name: 'Changed files' }));
/** The gutter is aria-hidden (CodeMirror), so its markers are found by line, not by role. */
const addMarker = (path: string, line: number) => document.querySelector<HTMLElement>(`[data-file="${path}"] [data-add-comment="${line}"]`)!;
const send = () => screen.getByRole('button', { name: /Send review to agent/ });

describe('Review', () => {
  it('all_files_shows_every_file', async () => {
    mount();
    await loaded();
    expect(sections()).toEqual([TOTALS, ROUND, SPEC]);
    expect(sidebar().getByRole('button', { name: /All files/ })).toHaveTextContent('3');
    // the list: status letter, short name, +/−
    const row = sidebar().getByRole('button', { name: /totals\.ts/ });
    expect(row).toHaveTextContent('M');
    expect(row).toHaveTextContent('+2');
    expect(row).toHaveTextContent('−1');
    expect(sidebar().getByRole('button', { name: /round\.ts/ })).toHaveTextContent('A');
  });

  it('file_click_shows_only_that_file', async () => {
    mount();
    await loaded();
    await userEvent.click(sidebar().getByRole('button', { name: /round\.ts/ }));
    await waitFor(() => expect(sections()).toEqual([ROUND]));
    expect(document.querySelector(`[data-file="${ROUND}"]`)).toHaveTextContent(ROUND);
    await userEvent.click(sidebar().getByRole('button', { name: /All files/ }));
    await waitFor(() => expect(sections()).toEqual([TOTALS, ROUND, SPEC]));
  });

  it('opens on the file it was asked for', async () => {
    mount({ file: SPEC });
    await loaded(1);
    expect(sections()).toEqual([SPEC]);
  });

  it('copy_patch_writes_clipboard: the patch of what is shown', async () => {
    mount();
    await loaded();
    await userEvent.click(screen.getByRole('button', { name: 'Copy patch' }));
    expect(writeText).toHaveBeenLastCalledWith(TOTALS_PATCH + ROUND_PATCH + SPEC_PATCH);
    await screen.findByText('Patch copied');
    await userEvent.click(sidebar().getByRole('button', { name: /totals\.spec\.ts/ }));
    await userEvent.click(screen.getByRole('button', { name: 'Copy patch' }));
    expect(writeText).toHaveBeenLastCalledWith(SPEC_PATCH);
  });

  it('Side by side and Unified switch the merge view', async () => {
    mount();
    await loaded();
    const split = screen.getByRole('button', { name: 'Side by side' });
    const unified = screen.getByRole('button', { name: 'Unified' });
    expect(split).toHaveAttribute('aria-pressed', 'true');
    expect(document.querySelector('.cm-mergeView')).not.toBeNull();
    await userEvent.click(unified);
    expect(unified).toHaveAttribute('aria-pressed', 'true');
    expect(split).toHaveAttribute('aria-pressed', 'false');
    await waitFor(() => expect(document.querySelector('.cm-mergeView')).toBeNull());
    await loaded();
    expect(document.querySelector(`[data-file="${TOTALS}"] .cm-deletedChunk`)?.textContent).toContain('Math.round(sum * 100) / 100');
  });

  it('shows the lines the agent removed and added', async () => {
    mount({ file: TOTALS });
    await loaded(1);
    const section = document.querySelector(`[data-file="${TOTALS}"]`)!;
    // side by side: the base on the left, the working tree on the right, the changed lines marked on both
    const [left, right] = [...section.querySelectorAll('.cm-mergeViewEditor')];
    expect(left?.textContent).toContain('Math.round(sum * 100) / 100');
    expect(left?.textContent).not.toContain('roundHalfEven(sum, 2)');
    expect([...right!.querySelectorAll('.cm-changedLine')].map((l) => l.textContent).join('\n')).toContain('return roundHalfEven(sum, 2)');
    expect(left?.querySelector('.cm-changedLine')?.textContent).toContain('Math.round(sum * 100) / 100');
  });

  it('esc_closes (the shell closes the review, also from a focused comment box)', async () => {
    selectSession('a1');
    const shell = render(
      <ClientProvider client={client}>
        <Shell />
      </ClientProvider>,
    );
    const open = () => document.querySelector('[data-slot="review"] [role="dialog"]');
    act(() => void emitUiCommand('openReview', 'a1', TOTALS));
    await waitFor(() => expect(open()).not.toBeNull());
    await waitFor(() => expect(addMarker(TOTALS, 41)).not.toBeNull());
    await userEvent.click(addMarker(TOTALS, 41));
    const box = await screen.findByRole('textbox', { name: 'Comment for line 41' });
    expect(box).toHaveFocus();
    fireEvent.keyDown(box, { key: 'Escape' });
    expect(open()).toBeNull();
    shell.unmount();
    act(() => {
      closeReview();
      selectSession(null);
    });
  });

  describe('comments', () => {
    it('a gutter click opens a box, typing counts, Remove deletes', async () => {
      mount({ file: TOTALS });
      await loaded(1);
      await userEvent.click(addMarker(TOTALS, 41));
      const box = await screen.findByRole('textbox', { name: 'Comment for line 41' });
      expect(box).toHaveFocus();
      expect(send()).toHaveTextContent('Send review to agent');
      expect(send()).not.toHaveTextContent('(');
      await userEvent.type(box, 'Why half-even?');
      expect(send()).toHaveTextContent('(1)');
      expect(getComments('a1')).toEqual([{ path: TOTALS, line: 41, quote: '  return roundHalfEven(sum, 2)', text: 'Why half-even?' }]);
      await userEvent.click(screen.getByRole('button', { name: 'Remove' }));
      expect(screen.queryByRole('textbox', { name: 'Comment for line 41' })).toBeNull();
      expect(getComments('a1')).toEqual([]);
    });

    it('⌘Return adds a comment on the cursor line (the gutter is mouse only)', async () => {
      mount({ file: TOTALS });
      await loaded(1);
      // the right editor is the working tree
      const view = EditorView.findFromDOM(document.querySelectorAll<HTMLElement>(`[data-file="${TOTALS}"] .cm-mergeViewEditor .cm-editor`)[1]!)!;
      act(() => view.dispatch({ selection: { anchor: view.state.doc.line(41).from } }));
      fireEvent.keyDown(view.contentDOM, { key: 'Enter', code: 'Enter', keyCode: 13, metaKey: true });
      await screen.findByRole('textbox', { name: 'Comment for line 41' });
    });

    it('are kept per session until sent: closing and opening the review keeps them', async () => {
      const first = mount({ file: TOTALS });
      await loaded(1);
      await userEvent.click(addMarker(TOTALS, 41));
      await userEvent.type(await screen.findByRole('textbox', { name: 'Comment for line 41' }), 'keep me');
      first.unmount();
      mount({ file: TOTALS });
      expect(await screen.findByRole('textbox', { name: 'Comment for line 41' })).toHaveValue('keep me');
      expect(send()).toHaveTextContent('(1)');
    });

    it('a box left empty is not a comment and is gone when the review closes', async () => {
      const first = mount({ file: TOTALS });
      await loaded(1);
      await userEvent.click(addMarker(TOTALS, 41));
      await screen.findByRole('textbox', { name: 'Comment for line 41' });
      first.unmount();
      expect(getComments('a1')).toEqual([]);
    });

    it('send_disabled_without_comments', async () => {
      mount({ file: TOTALS });
      await loaded(1);
      expect(send()).toBeDisabled();
      await userEvent.click(addMarker(TOTALS, 41)); // an empty box does not count
      await screen.findByRole('textbox', { name: 'Comment for line 41' });
      expect(send()).toBeDisabled();
      await userEvent.type(screen.getByRole('textbox', { name: 'Comment for line 41' }), 'x');
      expect(send()).toBeEnabled();
      await userEvent.clear(screen.getByRole('textbox', { name: 'Comment for line 41' }));
      expect(send()).toBeDisabled();
    });

    it('send_review_sends_one_prompt: every comment in one paste, then one Return, then cleared', async () => {
      const sent = vi.spyOn(client, 'sendInput');
      mount();
      await loaded();
      await userEvent.click(addMarker(TOTALS, 41));
      await userEvent.type(await screen.findByRole('textbox', { name: 'Comment for line 41' }), 'Why half-even?');
      await userEvent.click(addMarker(ROUND, 1));
      await userEvent.type(await screen.findByRole('textbox', { name: 'Comment for line 1' }), 'Add a doc comment');
      expect(send()).toHaveTextContent('(2)');
      await userEvent.click(send());
      await waitFor(() => expect(onClose).toHaveBeenCalledTimes(1));
      const texts = sent.mock.calls.map(([, text]) => text);
      expect(texts.filter((t) => t !== '\r')).toHaveLength(1); // exactly one prompt
      expect(texts.filter((t) => t === '\r')).toHaveLength(1); // submitted once
      expect(texts.at(-1)).toBe('\r');
      const prompt = (texts[0] ?? '').replace(/\r/g, '\n');
      expect(sent.mock.calls.every(([id]) => id === 'a1')).toBe(true);
      expect(prompt).toContain(`${TOTALS}:41\n> return roundHalfEven(sum, 2)\nWhy half-even?`);
      expect(prompt).toContain(`${ROUND}:1\n> export function roundHalfEven(n: number, dp: number) {\nAdd a doc comment`);
      expect(getComments('a1')).toEqual([]);
    });

    it('the review goes out as a bracketed paste although no terminal is mounted', async () => {
      const sent = vi.spyOn(client, 'sendInput');
      mount({ file: TOTALS });
      await loaded(1);
      await userEvent.click(addMarker(TOTALS, 41));
      await userEvent.type(await screen.findByRole('textbox', { name: 'Comment for line 41' }), 'why');
      await userEvent.click(send());
      await waitFor(() => expect(sent).toHaveBeenCalledTimes(2));
      const first = String(sent.mock.calls[0]?.[1]);
      expect(first.startsWith('\x1b[200~')).toBe(true);
      expect(first.endsWith('\x1b[201~')).toBe(true);
    });

    it('a send that fails keeps the comments and says so', async () => {
      vi.spyOn(client, 'sendInput').mockRejectedValue(new Error('daemon gone'));
      onClose.mockClear();
      mount({ file: TOTALS });
      await loaded(1);
      await userEvent.click(addMarker(TOTALS, 41));
      await userEvent.type(await screen.findByRole('textbox', { name: 'Comment for line 41' }), 'still here');
      await userEvent.click(send());
      expect(await screen.findByRole('alert')).toHaveTextContent('daemon gone');
      expect(getComments('a1')).toHaveLength(1);
      expect(onClose).not.toHaveBeenCalled();
      expect(send()).toBeEnabled();
    });
  });

  describe('states', () => {
    const only = (diff: GitDiff) => vi.spyOn(client, 'getDiff').mockResolvedValue(diff);

    it('no changes: says so, nothing to copy or send', async () => {
      only({ is_git: true, files: [], truncated: false });
      mount();
      expect(await screen.findByText('No changes to review')).toBeInTheDocument();
      expect(screen.getByRole('button', { name: 'Copy patch' })).toBeDisabled();
      expect(send()).toBeDisabled();
    });

    it('not a git folder: says so', async () => {
      only({ is_git: false, files: [], truncated: false });
      mount();
      expect(await screen.findByText(/not a git repository/i)).toBeInTheDocument();
    });

    it('a failed load shows the error', async () => {
      vi.spyOn(client, 'getDiff').mockRejectedValue(new Error('git exploded'));
      mount();
      expect(await screen.findByRole('alert')).toHaveTextContent('git exploded');
    });

    it('a truncated file and a truncated diff say so', async () => {
      only({ is_git: true, files: [file(SPEC, { diff: SPEC_PATCH, truncated: true })], truncated: true });
      mount();
      await loaded(1);
      expect(screen.getByText(/diff text of this file is cut/i)).toBeInTheDocument();
      expect(screen.getByRole('status')).toHaveTextContent('later files are left out');
    });

    it('a binary file is listed, not shown', async () => {
      only({ is_git: true, files: [file('docs/logo.png', { binary: true })], truncated: false });
      mount();
      expect(await screen.findByText('Binary file: not shown.')).toBeInTheDocument();
    });

    it('a file that cannot be read falls back to its patch text (no comments there)', async () => {
      only({ is_git: true, files: [file('src/gone.ts', { diff: '@@ -1 +1 @@\n-old line\n+new line\n' })], truncated: false });
      mount();
      expect(await screen.findByText('+new line')).toBeInTheDocument();
      expect(screen.getByText('-old line')).toBeInTheDocument();
      expect(document.querySelector('[data-add-comment]')).toBeNull();
    });
  });
});

describe('old side of the diff', () => {
  const commit = (sha: string) => ({ sha, subject: sha, author: 'a', at: 0 });
  const review = (over: Partial<Awaited<ReturnType<MockClient['getReview']>>>) =>
    vi.spyOn(client, 'getReview').mockResolvedValue({ base: 'origin/main', ahead: 2, behind: 3, commits: [commit('c2'), commit('c1')], truncated: false, ...over });

  it('is read at the merge base (parent of the oldest commit ahead), not at the base tip', async () => {
    review({});
    const read = vi.spyOn(client, 'readFile');
    mount({ file: TOTALS });
    await loaded(1);
    expect(read).toHaveBeenCalledWith('a1', TOTALS, 'c1^');
    expect(read).not.toHaveBeenCalledWith('a1', TOTALS, 'origin/main');
    expect(sidebar().getByText(/vs/)).toHaveTextContent('origin/main');
  });

  it('nothing ahead: the merge base is HEAD', async () => {
    review({ ahead: 0, commits: [] });
    const read = vi.spyOn(client, 'readFile');
    mount({ file: TOTALS });
    await loaded(1);
    expect(read).toHaveBeenCalledWith('a1', TOTALS, 'HEAD');
  });
});

describe('Review chrome', () => {
  it('Back closes, the title names the session, the base and counts are in the sidebar', async () => {
    onClose.mockClear();
    mount();
    await loaded();
    expect(screen.getByRole('dialog', { name: 'Review' })).toHaveTextContent('Fix invoice rounding');
    expect(sidebar().getByText(/vs/)).toHaveTextContent('main');
    expect(sidebar().getByText(/vs/)).toHaveTextContent('3 files');
    expect(sidebar().getByText(/vs/)).toHaveTextContent('+10');
    expect(sidebar().getByText(/vs/)).toHaveTextContent('−2');
    await userEvent.click(screen.getByRole('button', { name: /Back/ }));
    expect(onClose).toHaveBeenCalledTimes(1);
  });
});
