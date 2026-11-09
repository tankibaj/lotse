import { act, screen } from '@testing-library/react';
import { describe, expect, it } from 'vitest';
import { selectSession } from '../lib/ui/selection';
import { mount } from '../sidebar/testUtils';
import { StatusBar } from './StatusBar';

// Footer data from the Claude mod (proto 5): rate-limit meter and the selected session's context %.
// Without the mod both are absent and the footer is as before.

async function ready() {
  const view = mount(<StatusBar />);
  await screen.findByText(/lotsed running/);
  return view;
}

const HOUR = 3_600_000;

describe('rate-limit meter', () => {
  it('is hidden without rate limits', async () => {
    await ready();
    expect(screen.queryByTestId('status-rate')).toBeNull();
  });

  it('shows each window with its share, warns from 75 % and goes critical from 90 %', async () => {
    const { client } = await ready();
    act(() =>
      client.setRateLimits([
        { kind: 'five_hour', pct: 42, resets_at: Date.now() + 2 * HOUR },
        { kind: 'seven_day', pct: 80, resets_at: Date.now() + 50 * HOUR },
        { kind: 'opus_week', pct: 95, resets_at: Date.now() + HOUR },
      ]),
    );
    const meter = screen.getByTestId('status-rate');
    expect(meter).toHaveTextContent('5h 42%');
    expect(meter).toHaveTextContent('7d 80%');
    expect(meter).toHaveTextContent('opus_week 95%');
    const levels = [...meter.querySelectorAll('[data-window]')].map((w) => [w.getAttribute('data-window'), w.getAttribute('data-level')]);
    expect(levels).toEqual([
      ['five_hour', 'ok'],
      ['seven_day', 'warn'],
      ['opus_week', 'crit'],
    ]);
    expect(meter.querySelector('[data-window="five_hour"]')).toHaveAttribute('title', expect.stringMatching(/42% used.*resets in 2h/));
  });

  it('follows usage_changed and goes away when the mod stops reporting', async () => {
    const { client } = await ready();
    act(() => client.setRateLimits([{ kind: 'five_hour', pct: 10, resets_at: Date.now() + HOUR }]));
    expect(screen.getByTestId('status-rate')).toHaveTextContent('5h 10%');
    act(() => client.setRateLimits([]));
    expect(screen.queryByTestId('status-rate')).toBeNull();
  });
});

describe('context %', () => {
  it('shows the selected session’s context window use, and nothing without the mod', async () => {
    const { client } = await ready();
    act(() => selectSession('a1'));
    expect(screen.queryByTestId('status-context')).toBeNull();
    act(() => client.setSessionMeta('a1', { context_pct: 72 }));
    expect(screen.getByTestId('status-context')).toHaveTextContent('ctx 72%');
    act(() => selectSession('a5'));
    expect(screen.queryByTestId('status-context')).toBeNull();
  });
});
