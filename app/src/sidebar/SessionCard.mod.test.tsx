import { act, screen, within } from '@testing-library/react';
import { describe, expect, it } from 'vitest';
import type { SubAgent } from '../lib/client/types';
import { Sidebar } from './Sidebar';
import { mount } from './testUtils';

// Proto 5 data on the cards (context %, todo bar, source tooltip, sub-agent rows). Every field is absent without the mod:
// the card then looks as before.

const card = (id: string) => {
  const el = document.querySelector<HTMLElement>(`[data-sid="${id}"]`);
  if (!el) throw new Error(`no card ${id}`);
  return el;
};
const part = (id: string, name: string) => card(id).querySelector<HTMLElement>(`[data-part="${name}"]`);

async function ready() {
  const view = mount(<Sidebar />);
  await screen.findByText('billing-api');
  return view;
}

const sub = (id: string, name: string, over: Partial<SubAgent> = {}): SubAgent => ({ id, name, status: 'run', ...over });

describe('context %', () => {
  it('is absent without the mod', async () => {
    await ready();
    expect(part('a1', 'context')).toBeNull();
    expect(part('a1', 'todo')).toBeNull();
  });

  it('shows the share of the context window, and warns about compaction from 85 %', async () => {
    const { client } = await ready();
    act(() => client.setSessionMeta('a1', { context_pct: 72 }));
    expect(part('a1', 'context')).toHaveTextContent('72%');
    expect(part('a1', 'context')).toHaveAttribute('title', expect.stringContaining('72%'));
    expect(part('a1', 'context')).not.toHaveTextContent('compaction soon');
    act(() => client.setSessionMeta('a1', { context_pct: 91 }));
    expect(part('a1', 'context')).toHaveTextContent('91%');
    expect(part('a1', 'context')).toHaveAttribute('title', expect.stringContaining('compacts it soon'));
    expect(part('a1', 'context')).toHaveClass('text-orange');
    act(() => client.setSessionMeta('a1', { context_pct: undefined }));
    expect(part('a1', 'context')).toBeNull();
  });
});

describe('todo bar', () => {
  it('shows done of total as a progress bar, and nothing without a todo list', async () => {
    const { client } = await ready();
    act(() => client.setSessionMeta('a1', { todo: { done: 3, total: 7 } }));
    const bar = within(card('a1')).getByRole('progressbar', { name: 'Todo' });
    expect(bar).toHaveAttribute('aria-valuenow', '3');
    expect(bar).toHaveAttribute('aria-valuemax', '7');
    expect(part('a1', 'todo')).toHaveTextContent('3/7');
    act(() => client.setSessionMeta('a1', { todo: { done: 0, total: 0 } }));
    expect(part('a1', 'todo')).toBeNull();
  });
});

describe('source tooltip', () => {
  it('names the live source of the session: mod, hooks or screen', async () => {
    const { client } = await ready();
    expect(part('a1', 'agent')).not.toHaveAttribute('title');
    for (const layer of ['mod', 'hooks', 'screen'] as const) {
      act(() => client.setSessionMeta('a1', { source_layer: layer }));
      expect(part('a1', 'agent')).toHaveAttribute('title', expect.stringContaining(`Source: ${layer}`));
    }
  });
});

describe('sub-agent rows (proto 5)', () => {
  const rows = (id: string) => [...card(id).querySelectorAll<HTMLElement>('[data-sub-agent]')];

  it('show the activity line, tokens with cost, the model and a background marker', async () => {
    const { client } = await ready();
    act(() =>
      client.setSubAgents('a1', [
        sub('s1', 'test-runner', { activity: 'Read src/db/migrations', tokens: 12_400, cost_usd: 0.043, model: 'claude-haiku-5-5', background: true }),
        sub('s2', 'lint-fixer'),
      ]),
    );
    const [first, second] = rows('a1');
    expect(first?.querySelector('span')?.textContent).toBe('test-runner'); // the name stays the first span
    expect(first?.querySelector('[data-part="activity"]')).toHaveTextContent('Read src/db/migrations');
    expect(first?.querySelector('[data-part="usage"]')).toHaveTextContent('12k · $0.04');
    expect(first?.querySelector('[data-part="usage"]')).toHaveAttribute('title', expect.stringContaining('claude-haiku-5-5'));
    expect(first?.querySelector('[data-part="background"]')).not.toBeNull();
    expect(second?.querySelector('[data-part="activity"]')).toBeNull();
    expect(second?.querySelector('[data-part="usage"]')).toBeNull();
    expect(second?.querySelector('[data-part="background"]')).toBeNull();
  });

  it('nest a sub-agent under the one that started it', async () => {
    const { client } = await ready();
    act(() => client.setSubAgents('a1', [sub('c', 'child', { parent: 'p' }), sub('p', 'parent'), sub('o', 'other')]));
    expect(rows('a1').map((r) => [r.querySelector('span')?.textContent, r.getAttribute('data-depth')])).toEqual([
      ['parent', '0'],
      ['child', '1'],
      ['other', '0'],
    ]);
  });

  it('keep a sub-agent whose parent is unknown at the top level', async () => {
    const { client } = await ready();
    act(() => client.setSubAgents('a1', [sub('c', 'child', { parent: 'gone' })]));
    expect(rows('a1').map((r) => r.getAttribute('data-depth'))).toEqual(['0']);
  });
});
