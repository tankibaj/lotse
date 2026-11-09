import { act, screen, waitFor, within } from '@testing-library/react';
import { describe, expect, it, vi } from 'vitest';
import type { Usage, UsageSpan } from '../../lib/client/types';
import { ClientProvider, MockClient } from '../../lib/client';
import { mount } from '../../sidebar/testUtils';
import { UsagePanel } from './index';

const MIN = 60_000;
/** Local 14:02, so "since 14:02" holds in every time zone. */
const START = new Date(2026, 10, 3, 14, 2).getTime();
const span = (status: UsageSpan['status'], from: number, to: number): UsageSpan => ({ status, from: START + from * MIN, to: START + to * MIN });

const usage = (patch: Partial<Usage> = {}): Usage => ({
  started_at: START,
  total_ms: 41 * MIN,
  working_ms: 37 * MIN,
  waiting_ms: 4 * MIN,
  lanes: [
    { name: 'lead', kind: 'lead', spans: [span('run', 0, 3), span('perm', 3, 7), span('run', 7, 41)], cost_micros: 820_000 },
    { name: 'explore-repo', kind: 'sub_agent', spans: [span('done', 0.5, 9)], cost_micros: 190_000 },
    { name: 'test-runner', kind: 'sub_agent', spans: [span('run', 23, 41)], cost_micros: 130_000 },
  ],
  tokens: { input: 61_000, cache_read: 26_000, cache_write: 0, output: 9_000 },
  spend: { session: 1_140_000, project: 2_310_000, all: 4_180_000 },
  model: 'Opus 5.5',
  priced: true,
  ...patch,
});

function mountPanel(data: Usage, sessionId = 'a1') {
  const client = new MockClient({ script: false });
  const getUsage = vi.spyOn(client, 'getUsage').mockResolvedValue(data);
  const view = mount(<UsagePanel sessionId={sessionId} />, client);
  return { ...view, client, getUsage };
}

describe('UsagePanel', () => {
  it('tiles_show_time_split', async () => {
    mountPanel(usage());
    const session = await screen.findByTestId('usage-tile-session');
    expect(session).toHaveTextContent('Session');
    expect(session).toHaveTextContent('41m');
    expect(session).toHaveTextContent('since 14:02');
    expect(screen.getByTestId('usage-tile-working')).toHaveTextContent('37m');
    expect(screen.getByTestId('usage-tile-working')).toHaveTextContent('agent active');
    const waiting = screen.getByTestId('usage-tile-waiting');
    expect(waiting).toHaveTextContent('4m');
    expect(waiting).toHaveTextContent('on you');
    expect(within(waiting).getByText('4m')).toHaveStyle({ color: 'var(--orange)' });
    const split = screen.getByTestId('usage-split');
    expect(split).toHaveAttribute('title', 'Working 37m · Waiting on you 4m');
    expect(split.children[0]).toHaveStyle({ width: '90.24%', backgroundColor: 'var(--blue)' });
    expect(split.children[1]).toHaveStyle({ width: '9.76%', backgroundColor: 'var(--orange)' });
  });

  it('timeline_lane_per_sub_agent', async () => {
    mountPanel(usage());
    const lanes = await screen.findAllByTestId('usage-lane');
    expect(lanes.map((l) => l.getAttribute('data-kind'))).toEqual(['lead', 'sub_agent', 'sub_agent']);
    expect(lanes[0]).toHaveTextContent('lead');
    expect(lanes[0]).toHaveTextContent('41m · $0.82');
    expect(lanes[1]).toHaveTextContent('explore-repo');
    expect(lanes[1]).toHaveTextContent('8m · $0.19');
    expect(lanes[2]).toHaveTextContent('test-runner');
    expect(lanes[2]).toHaveTextContent('18m · $0.13');
    // one segment per span, coloured by status, placed on the 41 min axis
    const [leadLane, exploreLane] = lanes as [HTMLElement, HTMLElement, HTMLElement];
    const lead = within(leadLane).getAllByTestId('usage-span');
    expect(lead.map((s) => s.getAttribute('data-status'))).toEqual(['run', 'perm', 'run']);
    expect(lead[1]).toHaveStyle({ left: '7.32%', width: '9.76%', backgroundColor: 'var(--orange)' });
    expect(within(exploreLane).getByTestId('usage-span')).toHaveStyle({ backgroundColor: 'var(--green)' });
    expect(within(exploreLane).getByTestId('usage-span')).toHaveAttribute('title', 'Done · 8m');
    expect(screen.getByTestId('usage-axis')).toHaveTextContent('14:02');
    expect(screen.getByTestId('usage-axis')).toHaveTextContent('now');
  });

  it('tokens_only_when_unpriced', async () => {
    mountPanel(usage({ priced: false, spend: { session: 0, project: 0, all: 0 } }));
    const tokens = await screen.findByTestId('usage-tokens');
    expect(tokens).toHaveTextContent('96k');
    expect(tokens).toHaveTextContent('Input61k');
    expect(tokens).toHaveTextContent('Cache read26k');
    expect(tokens).toHaveTextContent('Output9k');
    expect(tokens).not.toHaveTextContent('$');
    expect(screen.queryByTestId('usage-spend')).not.toBeInTheDocument();
    for (const lane of screen.getAllByTestId('usage-lane')) expect(lane).not.toHaveTextContent('$');
    expect(screen.getByText(/without a known price/)).toBeInTheDocument();
  });

  it('shows tokens with their cost and the spend of session, project and all when priced', async () => {
    const { client } = mountPanel(usage());
    const project = (await client.listProjects()).find((p) => p.id === 'p1')!;
    expect(await screen.findByTestId('usage-tokens')).toHaveTextContent('96k · $1.14');
    const spend = screen.getByTestId('usage-spend');
    expect(spend).toHaveTextContent('This session$1.14');
    expect(spend).toHaveTextContent(`${project.name}$2.31`);
    expect(spend).toHaveTextContent('All projects$4.18');
    expect(screen.getByText(/estimates from local transcripts/)).toBeInTheDocument();
  });

  it('shows the time and "No token data" when the agent has no transcript', async () => {
    mountPanel(usage({ tokens: { input: 0, cache_read: 0, cache_write: 0, output: 0 }, spend: { session: 0, project: 0, all: 0 }, model: null }));
    expect(await screen.findByText('No token data')).toBeInTheDocument();
    expect(screen.getByTestId('usage-tile-session')).toHaveTextContent('41m');
    expect(screen.getAllByTestId('usage-lane')).toHaveLength(3);
    expect(screen.queryByTestId('usage-spend')).not.toBeInTheDocument();
    for (const lane of screen.getAllByTestId('usage-lane')) expect(lane).not.toHaveTextContent('$');
  });

  it('refreshes on usage_changed and on status events of its session, not of others', async () => {
    const { client, getUsage } = mountPanel(usage());
    await screen.findByTestId('usage-tile-session');
    expect(getUsage).toHaveBeenCalledTimes(1);
    getUsage.mockResolvedValue(usage({ total_ms: 42 * MIN }));
    act(() => client.setStatus('a5', 'fail'));
    expect(getUsage).toHaveBeenCalledTimes(1);
    act(() => client.setStatus('a1', 'perm'));
    await waitFor(() => expect(screen.getByTestId('usage-tile-session')).toHaveTextContent('42m'));
    getUsage.mockResolvedValue(usage({ total_ms: 43 * MIN }));
    act(() => client.setUsage({ input_tokens: 1, output_tokens: 1, cache_read_tokens: 0, cache_write_tokens: 0, cost_micros: 1 }));
    await waitFor(() => expect(screen.getByTestId('usage-tile-session')).toHaveTextContent('43m'));
  });

  it('asks again for the newly selected session', async () => {
    const { client, getUsage, rerender } = mountPanel(usage());
    await screen.findByTestId('usage-tile-session');
    expect(getUsage).toHaveBeenLastCalledWith('a1');
    rerender(
      <ClientProvider client={client}>
        <UsagePanel sessionId="a3" />
      </ClientProvider>,
    );
    await waitFor(() => expect(getUsage).toHaveBeenLastCalledWith('a3'));
  });

  it('says so when no session is selected', () => {
    mount(<UsagePanel sessionId={null} />);
    expect(screen.getByText('No session selected')).toBeInTheDocument();
  });

  it('shows the daemon error instead of tiles', async () => {
    const client = new MockClient({ script: false });
    vi.spyOn(client, 'getUsage').mockRejectedValue(new Error('not available for terminals'));
    mount(<UsagePanel sessionId="a7" />, client);
    expect(await screen.findByText(/not available for terminals/)).toBeInTheDocument();
    expect(screen.queryByTestId('usage-tile-session')).not.toBeInTheDocument();
  });
});
