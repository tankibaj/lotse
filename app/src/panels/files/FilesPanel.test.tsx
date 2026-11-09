import { screen, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { afterEach, describe, expect, it, vi } from 'vitest';
import { onUiCommand } from '../../lib/ui/commands';
import { mount } from '../../sidebar/testUtils';
import { FilesPanel } from './index';

afterEach(() => vi.restoreAllMocks());

/** A static MockClient session: a1 is a worktree with 3 changed files, a7 a non-git folder. */
async function ready(sessionId: string) {
  const view = mount(<FilesPanel sessionId={sessionId} />);
  await screen.findByRole('searchbox', { name: 'Filter files' });
  await screen.findByText(/^\d+\+? files/);
  return view;
}

const fileRow = (name: string) => screen.getByText(name).closest('[data-file]') as HTMLElement;
const names = () => [...document.querySelectorAll('[data-file]')].map((el) => el.getAttribute('data-file'));

describe('FilesPanel', () => {
  it('changed_toggle_filters', async () => {
    await ready('a1');
    expect(names()).toContain('src/invoice/tax.ts');
    await userEvent.click(screen.getByRole('tab', { name: 'Changed 3' }));
    expect(names()).toEqual(['src/invoice/totals.ts', 'src/invoice/round.ts', 'test/totals.spec.ts']);
    await userEvent.click(screen.getByRole('tab', { name: 'All' }));
    expect(names()).toContain('src/invoice/tax.ts');
  });

  it('filter_no_match_empty_state', async () => {
    await ready('a1');
    const input = screen.getByRole('searchbox', { name: 'Filter files' });
    await userEvent.type(input, 'zzz');
    expect(screen.getByText('No files match')).toBeInTheDocument();
    expect(input).toHaveFocus();
    await userEvent.clear(input);
    await userEvent.type(input, 'ROUND');
    expect(screen.queryByText('No files match')).toBeNull();
    expect(names()).toEqual(['src/invoice/round.ts', 'test/round.spec.ts']);
    expect(input).toHaveFocus();
  });

  it('click_emits_open_file', async () => {
    await ready('a1');
    const open = vi.fn();
    const off = onUiCommand('openFile', open);
    await userEvent.click(fileRow('totals.ts'));
    off();
    expect(open).toHaveBeenCalledWith('a1', 'src/invoice/totals.ts');
  });

  it('non_git_hides_toggle', async () => {
    await ready('a7');
    expect(screen.queryByRole('tab')).toBeNull();
    expect(screen.queryByText('Review changes')).toBeNull();
    expect(document.querySelector('[data-badge]')).toBeNull();
    expect(names().length).toBeGreaterThan(0);
  });

  it('shows +/−, the A/M/D badge and footer counts for changed files', async () => {
    await ready('a1');
    const totals = within(fileRow('totals.ts'));
    expect(totals.getByText('+2')).toBeInTheDocument();
    expect(totals.getByText('−1')).toBeInTheDocument();
    expect(totals.getByTitle('Modified')).toHaveTextContent('M');
    expect(within(fileRow('round.ts')).getByTitle('Added')).toHaveTextContent('A');
    expect(screen.getByText('10 files · 3 changed')).toBeInTheDocument();
  });

  it('dots a collapsed folder that holds changes, not an open one', async () => {
    await ready('a1');
    const folder = (n: string) => screen.getByText(n, { selector: 'span' }).closest('button') as HTMLElement;
    expect(within(folder('test')).getByTitle('Contains changes')).toBeInTheDocument();
    expect(within(folder('src')).queryByTitle('Contains changes')).toBeNull();
    await userEvent.click(folder('src'));
    expect(folder('src')).toHaveAttribute('aria-expanded', 'false');
    expect(within(folder('src')).getByTitle('Contains changes')).toBeInTheDocument();
    expect(names()).not.toContain('src/invoice/totals.ts');
  });

  it('Review changes opens the review of the session', async () => {
    await ready('a1');
    const review = vi.fn();
    const off = onUiCommand('openReview', review);
    await userEvent.click(screen.getByRole('button', { name: 'Review changes' }));
    off();
    expect(review).toHaveBeenCalledWith('a1');
  });

  it('Copy path writes the path to the clipboard', async () => {
    await ready('a1');
    const writeText = vi.fn().mockResolvedValue(undefined);
    Object.defineProperty(navigator, 'clipboard', { value: { writeText }, configurable: true });
    const open = vi.fn();
    const off = onUiCommand('openFile', open);
    await userEvent.click(within(fileRow('tax.ts')).getByRole('button', { name: 'Copy path' }));
    off();
    expect(writeText).toHaveBeenCalledWith('src/invoice/tax.ts');
    expect(open).not.toHaveBeenCalled();
  });
});
