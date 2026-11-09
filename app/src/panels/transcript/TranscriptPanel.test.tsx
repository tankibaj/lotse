import { act, fireEvent, render, screen, waitFor, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { afterEach, describe, expect, it, vi } from 'vitest';
import { ClientProvider, MockClient } from '../../lib/client';
import { DaemonError } from '../../lib/client/types';
import type { SessionId, Transcript, TranscriptTurn } from '../../lib/client/types';
import { resetUiState } from '../../lib/state';
import { selectSession } from '../../lib/ui/selection';
import { setExportBridge } from './exportBridge';
import type { ExportBridge } from './exportBridge';
import { TranscriptPanel } from './index';

const at = (min: number) => Date.UTC(2026, 10, 3, 14, min);
const tool = (name: string, summary: string, result: string, status: 'run' | 'done' | 'fail' = 'done', sub?: string): TranscriptTurn => ({
  role: 'tool',
  text: '',
  tool: { name, summary, result, status },
  at: at(3),
  ...(sub ? { sub_agent: sub } : {}),
});

const TURNS: TranscriptTurn[] = [
  { role: 'user', text: "Fix the flaky invoice rounding test and make totals use banker's rounding", tool: null, at: at(2) },
  { role: 'assistant', text: "I'll start by finding the rounding logic and the failing test.", tool: null, at: at(2) },
  tool('Task', 'explore-repo', '3 call sites in totals.ts', 'done', 'explore-repo'),
  tool('Read', 'src/invoice/totals.ts', '212 lines'),
  tool('Update', 'src/invoice/totals.ts', '+2 −1'),
  { role: 'approval', text: '', tool: null, at: at(5), approval: { decision: 'allow', via: 'notification', command: 'Bash(npm test -- totals)' } },
  { role: 'assistant', text: 'Totals now round half-even. Running the totals suite to confirm.', tool: null, at: at(5) },
  tool('Bash', 'npm test -- totals', '', 'run'),
  { role: 'assistant', text: 'failed: 2 lint errors', tool: null, at: at(6), sub_agent: 'lint-runner' },
];

const transcript = (turns = TURNS, source: Transcript['source'] = 'claude'): Transcript => ({ available: true, turns, truncated: false, source });

function mountPanel(opts: { sessionId?: SessionId; turns?: TranscriptTurn[] } = {}) {
  const client = new MockClient({ script: false });
  vi.spyOn(client, 'getTranscript').mockResolvedValue(transcript(opts.turns));
  selectSession(null);
  resetUiState();
  const view = render(
    <ClientProvider client={client}>
      <div data-testid="scroller" style={{ overflowY: 'auto' }}>
        <TranscriptPanel sessionId={opts.sessionId ?? 'a1'} />
      </div>
    </ClientProvider>,
  );
  return { client, ...view };
}

afterEach(() => setExportBridge(null));

const tab = (name: RegExp) => screen.getByRole('tab', { name });

describe('Transcript panel', () => {
  it('groups_consecutive_tools', async () => {
    mountPanel();
    await screen.findByText(/Fix the flaky invoice rounding test/);
    const groups = screen.getAllByTestId('tool-group');
    // Agent + Read + Update, then (after the approval and a message) Bash + the sub-agent's report
    expect(groups.map((g) => within(g).getAllByTestId('tool-row').length)).toEqual([3, 2]);
    const first = within(groups[0]!).getAllByTestId('tool-row');
    expect(first[0]).toHaveTextContent('Agent');
    expect(first[0]).toHaveTextContent('explore-repo');
    expect(first[0]).toHaveTextContent('3 call sites in totals.ts');
    expect(first[1]).toHaveTextContent('Read');
    expect(first[1]).toHaveTextContent('212 lines');
    const second = within(groups[1]!).getAllByTestId('tool-row');
    expect(second[0]).toHaveTextContent('Bash');
    expect(second[1]).toHaveTextContent('Agent');
    expect(second[1]).toHaveTextContent('lint-runner');
    expect(second[1]).toHaveTextContent('failed: 2 lint errors');
  });

  it('shows user bubbles right, agent turns with the agent name, and the orange approval card', async () => {
    mountPanel();
    const user = await screen.findByText(/Fix the flaky invoice rounding test/);
    expect(user.closest('[data-role="user"]')).toHaveClass('items-end');
    const agent = screen.getByText(/I'll start by finding/).closest('[data-role="assistant"]')!;
    expect(agent).toHaveTextContent(/^Claude · \d\d:\d\d/); // the mock says "Claude", not "Claude Code"
    const card = screen.getByTestId('approval-card');
    expect(card).toHaveTextContent('Permission allowed');
    expect(card).toHaveTextContent('Bash(npm test -- totals)');
    expect(card).toHaveTextContent('Allowed by you · notification');
  });

  it('approvals_filter', async () => {
    mountPanel();
    await screen.findByText(/Fix the flaky invoice rounding test/);
    expect(tab(/^All/)).toHaveTextContent('All');
    expect(tab(/^Messages/)).toHaveTextContent('Messages 3');
    expect(tab(/^Tools/)).toHaveTextContent('Tools 5');
    expect(tab(/^Approvals/)).toHaveTextContent('Approvals 1');

    await userEvent.click(tab(/^Approvals/));
    expect(screen.getByTestId('approval-card')).toBeInTheDocument();
    expect(screen.queryByText(/Fix the flaky invoice rounding test/)).not.toBeInTheDocument();
    expect(screen.queryByTestId('tool-group')).not.toBeInTheDocument();
    expect(tab(/^Approvals/)).toHaveAttribute('aria-selected', 'true');

    await userEvent.click(tab(/^Tools/));
    expect(screen.getAllByTestId('tool-row')).toHaveLength(5);
    expect(screen.queryByTestId('approval-card')).not.toBeInTheDocument();

    await userEvent.click(tab(/^Messages/));
    expect(screen.queryByTestId('tool-group')).not.toBeInTheDocument();
    expect(screen.getByText(/Totals now round half-even/)).toBeInTheDocument();
  });

  it('words a denied approval and one made in the terminal', async () => {
    const turns: TranscriptTurn[] = [
      { role: 'approval', text: '', tool: null, at: at(1), approval: { decision: 'deny', via: 'sidebar', command: 'Bash(rm -rf dist)' } },
      { role: 'approval', text: '', tool: null, at: at(2), approval: { decision: 'always', via: null, command: 'Edit(src/a.ts)' } },
    ];
    mountPanel({ turns });
    const cards = await screen.findAllByTestId('approval-card');
    expect(cards[0]).toHaveTextContent('Permission denied');
    expect(cards[0]).toHaveTextContent('Denied by you · sidebar');
    expect(cards[1]).toHaveTextContent('Always allowed');
    expect(cards[1]).toHaveTextContent('Always allowed in the terminal');
  });

  it('search_no_match_empty_state', async () => {
    mountPanel();
    await screen.findByText(/Fix the flaky invoice rounding test/);
    await userEvent.type(screen.getByRole('searchbox', { name: 'Search transcript' }), 'banker');
    expect(screen.getByText(/Fix the flaky invoice rounding test/)).toBeInTheDocument();
    expect(screen.queryByText(/Totals now round half-even/)).not.toBeInTheDocument();

    await userEvent.clear(screen.getByRole('searchbox', { name: 'Search transcript' }));
    await userEvent.type(screen.getByRole('searchbox', { name: 'Search transcript' }), 'zzz-nothing');
    expect(screen.getByText('No matches')).toBeInTheDocument();
    expect(screen.queryByTestId('tool-group')).not.toBeInTheDocument();
  });

  it('searches tool names, results and approval commands too', async () => {
    mountPanel();
    await screen.findByText(/Fix the flaky invoice rounding test/);
    await userEvent.type(screen.getByRole('searchbox', { name: 'Search transcript' }), 'call sites');
    expect(screen.getAllByTestId('tool-row')).toHaveLength(1);
    await userEvent.clear(screen.getByRole('searchbox', { name: 'Search transcript' }));
    await userEvent.type(screen.getByRole('searchbox', { name: 'Search transcript' }), 'npm test');
    expect(screen.getByTestId('approval-card')).toBeInTheDocument();
    expect(screen.getAllByTestId('tool-row')).toHaveLength(1);
  });

  it('⌘F focuses the search field', async () => {
    mountPanel();
    await screen.findByText(/Fix the flaky invoice rounding test/);
    fireEvent.keyDown(document, { key: 'f', metaKey: true });
    expect(screen.getByRole('searchbox', { name: 'Search transcript' })).toHaveFocus();
  });

  it('latest_button_scrolls_when_not_at_bottom', async () => {
    mountPanel(); // a1 runs: the Working bar shows
    await screen.findByText(/Fix the flaky invoice rounding test/);
    const scroller = screen.getByTestId('scroller');
    let top = 0;
    Object.defineProperty(scroller, 'scrollHeight', { configurable: true, value: 1000 });
    Object.defineProperty(scroller, 'clientHeight', { configurable: true, value: 300 });
    Object.defineProperty(scroller, 'scrollTop', { configurable: true, get: () => top, set: (v: number) => void (top = v) });

    const bar = await screen.findByTestId('working-bar');
    expect(bar).toHaveTextContent('Working');
    act(() => void fireEvent.scroll(scroller));
    await userEvent.click(within(bar).getByRole('button', { name: /Latest/ }));
    expect(top).toBe(1000);
  });

  it('has no Working bar while the session does not run', async () => {
    mountPanel({ sessionId: 'a2' }); // Codex, idle
    await screen.findByText(/Fix the flaky invoice rounding test/);
    expect(screen.queryByTestId('working-bar')).not.toBeInTheDocument();
  });

  it('names the agent and the model in the header', async () => {
    mountPanel({ sessionId: 'a2' });
    expect(await screen.findByText(/Codex · Opus 5.5/)).toBeInTheDocument();
  });

  it('says why an agent without transcript has none', async () => {
    const client = new MockClient({ script: false });
    vi.spyOn(client, 'getTranscript').mockRejectedValue(new DaemonError({ code: 'unavailable', message: 'Not available for OpenCode' }));
    render(
      <ClientProvider client={client}>
        <TranscriptPanel sessionId="a6" />
      </ClientProvider>,
    );
    expect(await screen.findByRole('alert')).toHaveTextContent('Not available for OpenCode');
    expect(screen.queryByRole('button', { name: 'Export' })).not.toBeInTheDocument();
  });

  it('reads a terminal\'s scrollback like any transcript', async () => {
    const client = new MockClient({ script: false });
    vi.spyOn(client, 'getTranscript').mockResolvedValue({ available: true, source: 'scrollback', truncated: true, turns: [{ role: 'assistant', text: '$ ls\nsrc  test', tool: null, at: null }] });
    render(
      <ClientProvider client={client}>
        <TranscriptPanel sessionId="a7" />
      </ClientProvider>,
    );
    expect(await screen.findByText(/\$ ls/)).toBeInTheDocument();
    expect(screen.getByText(/terminal scrollback/)).toBeInTheDocument();
    expect(screen.getByText('Older turns are not shown.')).toBeInTheDocument();
  });

  it('says so when the agent has not reported a transcript yet', async () => {
    const client = new MockClient({ script: false });
    vi.spyOn(client, 'getTranscript').mockResolvedValue({ available: false, turns: [], truncated: false });
    render(
      <ClientProvider client={client}>
        <TranscriptPanel sessionId="a1" />
      </ClientProvider>,
    );
    expect(await screen.findByText(/No transcript yet/)).toBeInTheDocument();
  });

  it('shows a null selection as an empty panel', () => {
    const client = new MockClient({ script: false });
    render(
      <ClientProvider client={client}>
        <TranscriptPanel sessionId={null} />
      </ClientProvider>,
    );
    expect(screen.getByText('No session selected')).toBeInTheDocument();
  });
});

describe('Transcript export', () => {
  const bridge = (path: string | null = '/Users/naim/Desktop/fix.md'): ExportBridge => ({ save: vi.fn().mockResolvedValue(path) });

  it('export_menu_calls_client_with_format', async () => {
    const { client } = mountPanel();
    const exp = vi.spyOn(client, 'exportTranscript').mockResolvedValue({ file_name: 'fix.md', content: '# Fix', redactions: 2 });
    const b = bridge();
    setExportBridge(b);
    await screen.findByText(/Fix the flaky invoice rounding test/);

    await userEvent.click(screen.getByRole('button', { name: 'Export' }));
    expect(screen.getByRole('menuitem', { name: 'Markdown (.md)' })).toBeInTheDocument();
    expect(screen.getByRole('menuitem', { name: 'JSON Lines (.jsonl)' })).toBeInTheDocument();
    expect(screen.getByRole('menuitem', { name: 'Terminal text (.txt)' })).toBeInTheDocument();
    expect(screen.getByText('Saved locally. Secrets redacted.')).toBeInTheDocument();
    await userEvent.click(screen.getByRole('menuitem', { name: 'Markdown (.md)' }));

    expect(exp).toHaveBeenCalledWith('a1', 'markdown');
    expect(b.save).toHaveBeenCalledWith({ fileName: 'fix.md', content: '# Fix' });
    expect(await screen.findByRole('status')).toHaveTextContent('Saved fix.md · 2 secrets redacted');
    expect(screen.queryByRole('menu')).not.toBeInTheDocument();

    await userEvent.click(screen.getByRole('button', { name: 'Export' }));
    await userEvent.click(screen.getByRole('menuitem', { name: 'JSON Lines (.jsonl)' }));
    expect(exp).toHaveBeenLastCalledWith('a1', 'json_lines');
    await userEvent.click(screen.getByRole('button', { name: 'Export' }));
    await userEvent.click(screen.getByRole('menuitem', { name: 'Terminal text (.txt)' }));
    expect(exp).toHaveBeenLastCalledWith('a1', 'text');
  });

  it('writes nothing and says nothing when the save dialog is cancelled', async () => {
    const { client } = mountPanel();
    vi.spyOn(client, 'exportTranscript').mockResolvedValue({ file_name: 'fix.md', content: '# Fix', redactions: 0 });
    const b = bridge(null);
    setExportBridge(b);
    await screen.findByText(/Fix the flaky invoice rounding test/);
    await userEvent.click(screen.getByRole('button', { name: 'Export' }));
    await userEvent.click(screen.getByRole('menuitem', { name: 'Markdown (.md)' }));
    await waitFor(() => expect(b.save).toHaveBeenCalled());
    expect(screen.queryByRole('status')).not.toBeInTheDocument();
    expect(screen.queryByRole('alert')).not.toBeInTheDocument();
  });

  it('shows the error when the daemon refuses or the write fails', async () => {
    const { client } = mountPanel();
    const exp = vi.spyOn(client, 'exportTranscript').mockRejectedValue(new DaemonError({ code: 'unavailable', message: 'Not available for OpenCode' }));
    await screen.findByText(/Fix the flaky invoice rounding test/);
    await userEvent.click(screen.getByRole('button', { name: 'Export' }));
    await userEvent.click(screen.getByRole('menuitem', { name: 'Terminal text (.txt)' }));
    expect(await screen.findByRole('alert')).toHaveTextContent('Not available for OpenCode');

    exp.mockResolvedValue({ file_name: 'fix.txt', content: 'x', redactions: 0 });
    setExportBridge({ save: vi.fn().mockRejectedValue('export: /x is a directory') });
    await userEvent.click(screen.getByRole('button', { name: 'Export' }));
    await userEvent.click(screen.getByRole('menuitem', { name: 'Terminal text (.txt)' }));
    expect(await screen.findByRole('alert')).toHaveTextContent('export: /x is a directory');
  });

  it('says "1 secret" in the singular', async () => {
    const { client } = mountPanel();
    vi.spyOn(client, 'exportTranscript').mockResolvedValue({ file_name: 'fix.md', content: '#', redactions: 1 });
    setExportBridge(bridge());
    await screen.findByText(/Fix the flaky invoice rounding test/);
    await userEvent.click(screen.getByRole('button', { name: 'Export' }));
    await userEvent.click(screen.getByRole('menuitem', { name: 'Markdown (.md)' }));
    expect(await screen.findByRole('status')).toHaveTextContent('1 secret redacted');
  });
});
