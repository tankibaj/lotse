import { act, fireEvent, screen, waitFor, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { describe, expect, it, vi } from 'vitest';
import { DaemonError } from '../lib/client/types';
import type { GitDiff } from '../lib/client/types';
import { emitUiCommand } from '../lib/ui/commands';
import { diffCounts, diffLines } from './diffLines';
import { mountWp5 } from './harness';
import { getOverlay } from './overlay';

describe('diffLines', () => {
  const diff = [
    'diff --git a/a.txt b/a.txt',
    'index 1..2 100644',
    '--- a/a.txt',
    '+++ b/a.txt',
    '@@ -1,3 +1,3 @@ fn x()',
    ' keep',
    '--- a removed line that starts with dashes',
    '+++ an added line that starts with pluses',
    '-gone',
    '+new',
    '\\ No newline at end of file',
    '',
  ].join('\n');

  it('tells headers from hunk content: inside a hunk the first character decides', () => {
    expect(diffLines(diff).map((l) => l.kind)).toEqual(['meta', 'meta', 'meta', 'meta', 'hunk', 'ctx', 'del', 'add', 'del', 'add', 'meta']);
    expect(diffCounts(diff)).toEqual({ added: 2, removed: 2 });
  });

  it('has no lines for an empty diff', () => {
    expect(diffLines('')).toEqual([]);
    expect(diffCounts('')).toEqual({ added: 0, removed: 0 });
  });
});

const open = (command: 'showChanges' | 'showTranscript', session: string) => act(() => void emitUiCommand(command, session));

describe('Show Changes sheet', () => {
  it('lists the files with status, counts and coloured unified diffs; the first three are open', async () => {
    mountWp5();
    open('showChanges', 'a1');
    const dlg = await screen.findByRole('dialog', { name: 'Changes' });
    expect(getOverlay()).toEqual({ name: 'changes', sessionId: 'a1' });
    expect(await within(dlg).findByText('src/invoice.ts')).toBeInTheDocument();
    expect(dlg).toHaveTextContent('Fix invoice rounding');
    expect(dlg).toHaveTextContent('3 changed files · 1 untracked');

    const invoice = dlg.querySelector<HTMLElement>('[data-file="src/invoice.ts"]')!;
    expect(within(invoice).getByRole('button')).toHaveAttribute('aria-expanded', 'true');
    expect(within(invoice).getByLabelText('Modified')).toHaveTextContent('M');
    expect(invoice).toHaveTextContent('+2');
    expect(invoice).toHaveTextContent('−1');
    const add = invoice.querySelector<HTMLElement>('[data-kind="add"]')!;
    const del = invoice.querySelector<HTMLElement>('[data-kind="del"]')!;
    expect(add.className).toMatch(/text-green/);
    expect(del.className).toMatch(/text-red/);
    expect(invoice.querySelector('[data-kind="hunk"]')?.className).toMatch(/text-blue/);
    expect(invoice.querySelector('[role="region"]')?.className).toMatch(/font-mono/);

    const binary = dlg.querySelector<HTMLElement>('[data-file="docs/logo.png"]')!;
    expect(binary).toHaveTextContent('binary');
    expect(binary).toHaveTextContent('Binary file: not shown.');

    // the fourth (untracked) file is collapsed until opened
    const notes = dlg.querySelector<HTMLElement>('[data-file="notes.md"]')!;
    expect(within(notes).getByRole('button')).toHaveAttribute('aria-expanded', 'false');
    expect(notes).not.toHaveTextContent('listed by name only');
    await userEvent.click(within(notes).getByRole('button'));
    expect(notes).toHaveTextContent('Not tracked by git: listed by name only.');
  });

  it('files collapse and expand, one by one and all at once', async () => {
    mountWp5();
    open('showChanges', 'a1');
    const dlg = await screen.findByRole('dialog', { name: 'Changes' });
    const invoice = await waitFor(() => {
      const el = dlg.querySelector<HTMLElement>('[data-file="src/invoice.ts"]');
      if (!el) throw new Error('not loaded');
      return el;
    });
    await userEvent.click(within(invoice).getByRole('button'));
    expect(within(invoice).getByRole('button')).toHaveAttribute('aria-expanded', 'false');
    expect(invoice.querySelector('[data-kind]')).toBeNull();
    await userEvent.click(within(dlg).getByRole('button', { name: 'Collapse all' }));
    expect(dlg.querySelectorAll('[aria-expanded="true"]')).toHaveLength(0);
    await userEvent.click(within(dlg).getByRole('button', { name: 'Expand all' }));
    expect(dlg.querySelectorAll('[aria-expanded="true"]')).toHaveLength(4);
  });

  it('says so for a folder without git, a clean one, an error and a cut diff', async () => {
    const { client } = mountWp5();
    open('showChanges', 'a7'); // Terminal in the notes folder
    expect(await screen.findByText(/No git: this folder is not a git repository/)).toBeInTheDocument();

    const spy = vi.spyOn(client, 'getDiff');
    spy.mockResolvedValue({ is_git: true, files: [], truncated: false });
    await userEvent.click(screen.getByRole('button', { name: 'Refresh' }));
    expect(await screen.findByText('No changes.')).toBeInTheDocument();

    spy.mockRejectedValue(new DaemonError({ code: 'unavailable', message: 'git failed: boom' }));
    await userEvent.click(screen.getByRole('button', { name: 'Refresh' }));
    expect(await screen.findByRole('alert')).toHaveTextContent('git failed: boom');

    const cut: GitDiff = {
      is_git: true,
      truncated: true,
      files: [{ path: 'big.txt', old_path: null, status: 'added', binary: false, truncated: true, diff: 'diff --git a/big.txt b/big.txt\n@@ -0,0 +1 @@\n+x\n' }],
    };
    spy.mockResolvedValue(cut);
    await userEvent.click(screen.getByRole('button', { name: 'Refresh' }));
    expect(await screen.findByRole('status')).toHaveTextContent('cut at 512 KB');
    expect(screen.getByText('cut')).toBeInTheDocument();
    expect(screen.getByText(/rest of this file's diff is cut/)).toBeInTheDocument();
  });

  it('shows a rename with its old path, closes with Esc and is read-only (no inputs)', async () => {
    const { client } = mountWp5();
    vi.spyOn(client, 'getDiff').mockResolvedValue({
      is_git: true,
      truncated: false,
      files: [{ path: 'new.ts', old_path: 'old.ts', status: 'renamed', binary: false, truncated: false, diff: 'diff --git a/old.ts b/new.ts\nsimilarity index 100%\n' }],
    });
    open('showChanges', 'a1');
    const dlg = await screen.findByRole('dialog', { name: 'Changes' });
    expect(await within(dlg).findByText(/← old\.ts/)).toBeInTheDocument();
    expect(dlg.querySelectorAll('input, textarea')).toHaveLength(0);
    fireEvent.keyDown(window, { key: 'Escape' });
    expect(getOverlay()).toBeNull();
  });
});

describe('Show Transcript sheet', () => {
  it('shows the turns: who spoke, tool calls with their summary, newest last', async () => {
    mountWp5();
    open('showTranscript', 'a1');
    const dlg = await screen.findByRole('dialog', { name: 'Transcript' });
    expect(getOverlay()).toEqual({ name: 'transcript', sessionId: 'a1' });
    expect(await within(dlg).findByText(/Fix the invoice rounding/)).toBeInTheDocument();
    expect(dlg).toHaveTextContent('Fix invoice rounding · Claude Code');
    const roles = [...dlg.querySelectorAll('[data-role]')].map((el) => el.getAttribute('data-role'));
    expect(roles).toEqual(['user', 'assistant', 'tool', 'tool', 'assistant', 'tool']);
    expect(within(dlg).getByText('You')).toBeInTheDocument();
    const tool = [...dlg.querySelectorAll<HTMLElement>('[data-role="tool"]')].at(-1)!;
    expect(tool).toHaveTextContent('Bash');
    expect(tool).toHaveTextContent('npm test -- totals');
    expect(tool.className).toMatch(/font-mono/);
    expect(dlg.querySelectorAll('input, textarea')).toHaveLength(0);
  });

  it('says why there is nothing: no hook yet, an agent without a transcript, older turns left out', async () => {
    const { client } = mountWp5();
    const spy = vi.spyOn(client, 'getTranscript');
    spy.mockResolvedValue({ available: false, turns: [], truncated: false });
    open('showTranscript', 'a1');
    expect(await screen.findByText(/No transcript yet/)).toBeInTheDocument();

    spy.mockRejectedValue(new DaemonError({ code: 'unavailable', message: 'Not available for OpenCode' }));
    await userEvent.click(screen.getByRole('button', { name: 'Refresh' }));
    expect(await screen.findByRole('alert')).toHaveTextContent('Not available for OpenCode');

    spy.mockResolvedValue({ available: true, turns: [{ role: 'user', text: 'hello', tool: null, at: null }], truncated: true });
    await userEvent.click(screen.getByRole('button', { name: 'Refresh' }));
    expect(await screen.findByText('hello')).toBeInTheDocument();
    expect(screen.getByRole('status')).toHaveTextContent('Older turns are not shown.');
  });

  it('closes with the ✕ button', async () => {
    mountWp5();
    open('showTranscript', 'a2');
    await screen.findByRole('dialog', { name: 'Transcript' });
    await userEvent.click(screen.getByRole('button', { name: 'Close' }));
    expect(getOverlay()).toBeNull();
  });
});
