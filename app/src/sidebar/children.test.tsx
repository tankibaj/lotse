import { act, screen } from '@testing-library/react';
import { describe, expect, it } from 'vitest';
import type { Session } from '../lib/client/types';
import { childOutcome, nestChildren } from './children';
import { selectSession } from '../lib/ui/selection';
import { Sidebar } from './Sidebar';
import { mount } from './testUtils';

const s = (id: string, parent?: string, status: Session['status'] = 'run') => ({ id, parent_session: parent, status }) as unknown as Session;

describe('nestChildren', () => {
  it('puts children right under their parent and marks them', () => {
    const out = nestChildren([s('a'), s('b'), s('k1', 'a'), s('k2', 'a'), s('g', 'k1')]);
    expect(out.map((o) => [o.session.id, o.child])).toEqual([
      ['a', false],
      ['k1', true],
      ['g', true],
      ['k2', true],
      ['b', false],
    ]);
  });

  it('a child whose parent is not in the list (closed, other project) is top level', () => {
    expect(nestChildren([s('k', 'gone'), s('a')]).map((o) => [o.session.id, o.child])).toEqual([
      ['k', false],
      ['a', false],
    ]);
  });

  it('survives a parent cycle', () => {
    expect(nestChildren([s('x', 'y'), s('y', 'x')]).map((o) => o.session.id).sort()).toEqual(['x', 'y']);
  });
});

describe('childOutcome', () => {
  it('counts done and failed children of one parent', () => {
    const all = [s('k1', 'a', 'done'), s('k2', 'a', 'fail'), s('k3', 'a', 'run'), s('k4', 'b', 'done')];
    expect(childOutcome('a', all)).toEqual({ done: 1, fail: 1 });
    expect(childOutcome('c', all)).toEqual({ done: 0, fail: 0 });
  });
});

describe('sidebar', () => {
  const card = (id: string) => document.querySelector<HTMLElement>(`[data-sid="${id}"]`);

  async function ready() {
    const view = mount(<Sidebar />);
    await screen.findByText('billing-api');
    return view;
  }

  it('shows a child under its parent with "started by", and moves it to the top level when the parent closes', async () => {
    const { client } = await ready();
    const parent = (await client.listSessions()).find((x) => x.id === 'a1');
    const kid = await client.createSession({
      project_id: parent?.project_id ?? 'p1',
      agent: 'claude_code',
      command: null,
      title: 'Child task',
      worktree: null,
      first_prompt: 'go',
      parent_session: 'a1',
      requested_by_agent: true,
    });
    const el = await screen.findByText('Child task').then(() => card(kid.id));
    expect(el).not.toBeNull();
    expect(el?.previousElementSibling?.getAttribute('data-sid')).toBe('a1');
    act(() => selectSession(kid.id)); // the details open on the selected session
    expect(card(kid.id)?.querySelector('[data-part="started-by"]')).toHaveTextContent(`started by ${parent?.title}`);
    act(() => client.setSessionMeta(kid.id, { parent_session: undefined }));
    expect(card(kid.id)?.querySelector('[data-part="started-by"]')).toBeNull();
  });

  it('badges the parent card when a child is done or failed', async () => {
    const { client } = await ready();
    const kid = await client.createSession({
      project_id: 'p1',
      agent: 'claude_code',
      command: null,
      title: 'Child task',
      worktree: null,
      first_prompt: 'go',
      parent_session: 'a1',
      requested_by_agent: true,
    });
    expect(card('a1')?.querySelector('[data-part="child-badge"]')).toBeNull();
    act(() => client.setStatus(kid.id, 'done'));
    expect(card('a1')?.querySelector('[data-part="child-badge"]')).toHaveTextContent('1 child done');
    act(() => client.setStatus(kid.id, 'fail'));
    expect(card('a1')?.querySelector('[data-part="child-badge"]')).toHaveTextContent('1 child failed');
  });
});
