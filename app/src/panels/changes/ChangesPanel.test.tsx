import { act, screen, waitFor, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { afterEach, describe, expect, it, vi } from 'vitest';
import { MockClient } from '../../lib/client/MockClient';
import type { DaemonEvent } from '../../lib/client/types';
import { closeReview, getReview } from '../../lib/ui/panel';
import { mount } from '../../sidebar/testUtils';
import { ChangesPanel } from './index';
import { mergeCommand } from './pr';

// Mock sessions (crates/lotse-proto/fixtures/snapshot_mock.json): a1 = billing-api worktree `lotse/fix-rounding`
// (↑2 ↓1, 3 dirty, draft PR #184), a2 = clean worktree, a3 = praxis-web (no PR), a7 = `notes`, not a git repository.

afterEach(() => {
  vi.restoreAllMocks();
  closeReview();
});

async function open(sessionId: string, client = new MockClient({ script: false })) {
  const view = mount(<ChangesPanel sessionId={sessionId} />, client);
  await screen.findByTestId('panel-changes');
  return view;
}

const fileRow = (path: string) => screen.findByRole('button', { name: `Open diff of ${path}` });

describe('ChangesPanel', () => {
  it('lists_files_with_counts', async () => {
    await open('a1');
    const totals = await screen.findByLabelText('4 changed files, 5 added, 1 removed');
    expect(totals).toHaveTextContent('4');
    expect(totals).toHaveTextContent('+5');
    expect(totals).toHaveTextContent('−1');

    const invoice = within(await fileRow('src/invoice.ts'));
    expect(invoice.getByText('invoice.ts')).toBeInTheDocument();
    expect(invoice.getByText('src/')).toBeInTheDocument();
    expect(invoice.getByLabelText('Modified')).toHaveTextContent('M');
    expect(invoice.getByText('+2')).toBeInTheDocument();
    expect(invoice.getByText('−1')).toBeInTheDocument();

    const rounding = within(await fileRow('src/rounding.ts'));
    expect(rounding.getByLabelText('Added')).toHaveTextContent('A');
    expect(rounding.getByText('+3')).toBeInTheDocument();
    expect(rounding.queryByText(/^−/)).toBeNull();

    expect(within(await fileRow('docs/logo.png')).getByText('binary')).toBeInTheDocument();
    expect(within(await fileRow('notes.md')).getByLabelText('Untracked')).toHaveTextContent('A');
  });

  it('shows the branch with ahead, behind and the PR pill that opens the PR', async () => {
    const open_ = vi.spyOn(window, 'open').mockReturnValue(null);
    await open('a1');
    expect(await screen.findByText('fix-rounding')).toHaveAttribute('title', 'lotse/fix-rounding');
    expect(screen.getByText('↑2')).toBeInTheDocument();
    expect(screen.getByText('↓1')).toBeInTheDocument();
    await userEvent.click(screen.getByRole('button', { name: /Open pull request/ }));
    expect(open_).toHaveBeenCalledWith('https://github.com/acme/billing-api/pull/184', '_blank', 'noopener,noreferrer');
    expect(screen.getByRole('button', { name: /Open pull request/ })).toHaveTextContent('PR #184 · Draft');
  });

  it('shows no PR pill without a PR', async () => {
    await open('a3');
    await fileRow('src/invoice.ts');
    expect(screen.queryByRole('button', { name: /Open pull request/ })).toBeNull();
  });

  it('click_file_opens_review_for_file', async () => {
    await open('a1');
    await userEvent.click(await fileRow('src/rounding.ts'));
    expect(getReview()).toEqual({ sessionId: 'a1', file: 'src/rounding.ts' });
  });

  it('Review full diff opens the review without a file', async () => {
    await open('a1');
    await userEvent.click(await screen.findByRole('button', { name: /Review full diff/ }));
    expect(getReview()).toEqual({ sessionId: 'a1' });
  });

  it('commits_ahead_listed', async () => {
    await open('a1');
    expect(await screen.findByText('Commits ahead of main')).toBeInTheDocument();
    expect(screen.getByText("Use banker's rounding for invoice totals")).toBeInTheDocument();
    expect(screen.getByText('a41c9e2')).toBeInTheDocument();
    expect(screen.getByText('Add roundHalfEven helper')).toBeInTheDocument();
    expect(screen.getByText('7be03d1')).toBeInTheDocument();
    expect(screen.getByText('4m')).toBeInTheDocument();
  });

  it('copy_merge_command', async () => {
    const writeText = vi.fn().mockResolvedValue(undefined);
    Object.defineProperty(navigator, 'clipboard', { value: { writeText }, configurable: true });
    await open('a1');
    expect(await screen.findByText('git merge lotse/fix-rounding')).toBeInTheDocument();
    await userEvent.click(screen.getByRole('button', { name: 'Copy merge command' }));
    expect(writeText).toHaveBeenCalledWith('git merge lotse/fix-rounding');
    expect(await screen.findByText('Copied')).toBeInTheDocument();
  });

  it('shell-quotes a branch name that is not plain', () => {
    expect(mergeCommand('lotse/fix-rounding')).toBe('git merge lotse/fix-rounding');
    expect(mergeCommand('lotse/x;rm -rf ~')).toBe("git merge 'lotse/x;rm -rf ~'");
    expect(mergeCommand("a'b")).toBe(`git merge 'a'\\''b'`);
  });

  it('offers no merge command in the main checkout', async () => {
    await open('a5');
    await fileRow('src/invoice.ts');
    expect(screen.queryByRole('button', { name: 'Copy merge command' })).toBeNull();
  });

  it('no_git_empty_state', async () => {
    await open('a7');
    expect(await screen.findByText('notes is not a git repository.')).toBeInTheDocument();
    expect(screen.queryByRole('button', { name: /Review full diff/ })).toBeNull();
  });

  it('a clean worktree says "No changes"', async () => {
    await open('a2');
    expect(await screen.findByText('No changes')).toBeInTheDocument();
    expect(screen.queryByRole('button', { name: /Review full diff/ })).toBeNull();
    expect(screen.queryByText(/^Commits ahead/)).toBeNull();
  });

  it('says so when no session is selected', () => {
    mount(<ChangesPanel sessionId={null} />);
    expect(screen.getByText('No session selected.')).toBeInTheDocument();
  });

  it('shows the error and retries', async () => {
    const client = new MockClient({ script: false });
    const get = vi.spyOn(client, 'getDiff').mockRejectedValueOnce(new Error('daemon down'));
    await open('a1', client);
    expect(await screen.findByRole('alert')).toHaveTextContent('daemon down');
    await userEvent.click(screen.getByRole('button', { name: 'Retry' }));
    expect(await fileRow('src/invoice.ts')).toBeInTheDocument();
    expect(get).toHaveBeenCalledTimes(2);
  });

  it('reloads when the session git state changes', async () => {
    const client = new MockClient({ script: false });
    const get = vi.spyOn(client, 'getDiff');
    await open('a2', client);
    expect(await screen.findByText('No changes')).toBeInTheDocument();
    expect(get).toHaveBeenCalledTimes(1);
    // `emit` is private; the store applies the event exactly as it would one from the daemon
    const emit = (client as unknown as { emit(e: DaemonEvent): void }).emit.bind(client);
    await act(async () => {
      emit({
        event: 'git_info_changed',
        data: { session_id: 'a2', git: { is_git: true, branch: 'lotse/flyway-v42', ahead: 0, behind: 0, dirty: 3, added: 5, removed: 1, worktree: true, worktree_path: null, pr: null } },
      });
    });
    await waitFor(() => expect(get).toHaveBeenCalledTimes(2));
  });
});
