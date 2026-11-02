import { act, fireEvent, screen, waitFor, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { afterEach, describe, expect, it, vi } from 'vitest';
import { MockClient, SUB_AGENT_SCRIPT } from '../lib/client/MockClient';
import { toggleNeedFilter } from '../lib/state';
import { onUiCommand } from '../lib/ui/commands';
import { getSelectedSessionId, selectSession } from '../lib/ui/selection';
import { hostBridge, setHostBridge } from '../sheets/hostBridge';
import { StatusBar } from '../statusbar';
import { Sidebar } from './Sidebar';
import { mount } from './testUtils';

const card = (id: string) => {
  const el = document.querySelector<HTMLElement>(`[data-sid="${id}"]`);
  if (!el) throw new Error(`no card ${id}`);
  return el;
};
const row = (name: string | RegExp) => screen.getByRole('button', { name });
const cardMain = (id: string) => {
  const el = card(id).querySelector<HTMLElement>('[data-card-main]');
  if (!el) throw new Error(`card ${id} has no main button`);
  return el;
};

async function ready() {
  const view = mount(<Sidebar />);
  await screen.findByText('billing-api');
  return view;
}

afterEach(() => vi.restoreAllMocks());

describe('tree', () => {
  it('shows the mock data: 5 projects, 7 sessions, first session selected', async () => {
    await ready();
    for (const p of ['billing-api', 'praxis-web', 'infra-terraform', 'dotfiles', 'notes']) {
      expect(screen.getByText(p)).toBeInTheDocument();
    }
    expect(document.querySelectorAll('[data-sid]')).toHaveLength(7);
    expect(card('a1')).toHaveAttribute('aria-current', 'true');
    expect(card('a2')).not.toHaveAttribute('aria-current');
  });

  it('card line 2: agent, branch without lotse/ prefix, ahead and dirty; non-git shows the agent only', async () => {
    await ready();
    const a1 = within(card('a1'));
    expect(a1.getByText('Claude Code')).toBeInTheDocument();
    expect(a1.getByText('fix-rounding')).toHaveAttribute('title', 'lotse/fix-rounding');
    expect(a1.getByText('↑2')).toBeInTheDocument();
    expect(a1.getByText('±3')).toBeInTheDocument();
    const a7 = within(card('a7'));
    expect(a7.getByText('Terminal')).toBeInTheDocument();
    expect(card('a7').querySelector('.font-mono')).toBeNull();
    expect(screen.getAllByRole('img', { name: 'Not a git repository' })).toHaveLength(1);
  });

  it('uses coarse time: minutes and hours, never seconds', async () => {
    await ready();
    expect(within(card('a1')).getByText('2m')).toBeInTheDocument(); // running 161 s
    expect(within(card('a2')).getByText('1h')).toBeInTheDocument(); // idle since 1 h
    expect(card('a1').textContent).not.toMatch(/\d+s\b/);
  });

  it('collapses and expands a project and remembers it', async () => {
    await ready();
    await userEvent.click(row(/billing-api/));
    expect(document.querySelector('[data-sid="a1"]')).toBeNull();
    expect(row(/billing-api/)).toHaveAttribute('aria-expanded', 'false');
    expect(JSON.parse(localStorage.getItem('lotse-collapsed') ?? '[]')).toEqual(['p1']);
    await userEvent.click(row(/billing-api/));
    expect(card('a1')).toBeInTheDocument();
  });

  it('"No sessions · New session" opens the new-session dropdown for that project, not the current one', async () => {
    const { client } = await ready();
    const sheet = vi.fn();
    const off = onUiCommand('openNewSession', sheet);
    const create = vi.spyOn(client, 'createSession');
    const empty = await act(() => client.addProject({ path: '/x/empty', name: 'empty', color: null, icon: null }));
    expect(await screen.findByText(/No sessions/)).toBeInTheDocument();
    expect(getSelectedSessionId()).toBe('a1'); // billing-api is current, the empty project is not
    await userEvent.click(screen.getByRole('button', { name: 'New session' }));
    expect(sheet).not.toHaveBeenCalled(); // a dropdown, not the modal
    const menu = screen.getByRole('menu', { name: 'New session' });
    expect(within(menu).getByText('empty')).toBeInTheDocument(); // headed with the project
    expect(within(menu).getByRole('menuitem', { name: /Worktree or other project…/ })).toBeInTheDocument();
    await userEvent.click(within(menu).getByRole('menuitem', { name: /^Codex/ }));
    expect(create).toHaveBeenCalledWith(expect.objectContaining({ project_id: empty.id, agent: 'codex', worktree: null }));
    await waitFor(() => expect(getSelectedSessionId()).not.toBe('a1'));

    // "Worktree or other project…" hands the same project to the sheet
    const other = await act(() => client.addProject({ path: '/x/empty2', name: 'empty2', color: null, icon: null }));
    await userEvent.click(await screen.findByRole('button', { name: 'New session' })); // `empty` has a session now
    await userEvent.click(screen.getByRole('menuitem', { name: /Worktree or other project…/ }));
    expect(sheet).toHaveBeenCalledTimes(1);
    expect(sheet).toHaveBeenCalledWith(other.id);
    off();
  });
});

describe('roll-up (collapsed only)', () => {
  it('shows nothing for an expanded project', async () => {
    await ready();
    expect(document.querySelector('[data-rollup]')).toBeNull();
  });

  it('collapsed: needs-you glyphs first (permission and failure), no digit for one', async () => {
    await ready();
    await userEvent.click(row(/praxis-web/));
    const r = row(/praxis-web/);
    expect(r.querySelector('[data-rollup="perm"]')?.textContent).toBe('Needs permission'); // glyph title only, no digit
    expect(r.querySelector('[data-rollup="fail"]')).not.toBeNull();
    expect(r.querySelector('[data-rollup="run"]')).toBeNull();
  });

  it('collapsed: counts above one, and the top state when nothing needs you', async () => {
    const { client } = await ready();
    await userEvent.click(row(/billing-api/));
    expect(row(/billing-api/).querySelector('[data-rollup="run"]')).not.toBeNull();
    act(() => client.setStatus('a2', 'perm', { tool: 'Bash', summary: 'ls' }));
    act(() => client.setStatus('a1', 'perm', { tool: 'Bash', summary: 'ls' }));
    expect(row(/billing-api/).querySelector('[data-rollup="perm"]')?.textContent).toBe('Needs permission2');
    expect(row(/billing-api/).querySelector('[data-rollup="run"]')).toBeNull();

    await userEvent.click(row(/dotfiles/));
    expect(row(/dotfiles/).querySelector('[data-rollup="done"]')).not.toBeNull();
    await userEvent.click(row(/notes/));
    expect(row(/notes/).querySelector('[data-rollup]')).toBeNull(); // idle shows nothing
  });
});

const glyph = (id: string) => {
  const el = card(id).querySelector<SVGElement>('[data-status]');
  if (!el) throw new Error(`card ${id} has no status glyph`);
  return el;
};

describe('status glyphs', () => {
  it('every status has its own icon, an accessible label and a plain-words tooltip', async () => {
    const { client } = await ready();
    const s = await client.createSession({ project_id: 'p1', agent: 'claude_code', command: null, title: null, worktree: null, first_prompt: null });
    await waitFor(() => expect(document.querySelector(`[data-sid="${s.id}"]`)).not.toBeNull());
    const seen: Record<string, string> = {
      a1: 'Running',
      a3: 'Needs permission',
      a4: 'Failed',
      a6: 'Done',
      a2: 'Idle',
      [s.id]: 'Detecting…',
    };
    const shapes = new Set<string>();
    for (const [id, label] of Object.entries(seen)) {
      const g = glyph(id);
      expect(g).toHaveAccessibleName(label);
      expect(g.querySelector('title')?.textContent).toBe(label);
      shapes.add(g.innerHTML);
    }
    expect(shapes.size).toBe(6);
    expect(glyph(s.id).getAttribute('class')).not.toMatch(/spin/); // unknown is not a spinner
    expect(glyph('a1').getAttribute('class')).toMatch(/spin/);
  });

  it('unknown agent session: no hint at first, "No live status yet" after 10 s; terminals never', async () => {
    const { client } = await ready();
    const s = await client.createSession({ project_id: 'p1', agent: 'codex', command: null, title: null, worktree: null, first_prompt: null });
    const t = await client.createSession({ project_id: 'p5', agent: 'terminal', command: null, title: null, worktree: null, first_prompt: null });
    await waitFor(() => expect(document.querySelector(`[data-sid="${s.id}"]`)).not.toBeNull());
    expect(within(card(s.id)).queryByText('No live status yet')).toBeNull();
    vi.spyOn(Date, 'now').mockReturnValue(Date.now() + 11_000);
    await waitFor(() => expect(within(card(s.id)).getByText('No live status yet')).toBeInTheDocument(), { timeout: 2500 });
    expect(within(card(s.id)).queryByRole('button', { name: /live status|hook|install/i })).toBeNull(); // no config-writing buttons
    expect(within(card(t.id)).queryByText('No live status yet')).toBeNull();
    expect(within(card('a2')).queryByText('No live status yet')).toBeNull();
  });
});

describe('sub-agent rows', () => {
  const scripted = async () => {
    const client = new MockClient({ script: SUB_AGENT_SCRIPT, tickMs: 0 });
    const view = mount(<Sidebar />, client);
    await screen.findByText('billing-api');
    return view;
  };
  const rows = (id: string) => [...card(id).querySelectorAll('[data-sub-agent]')].map((r) => [r.querySelector('span')?.textContent, r.querySelector('[data-status]')?.getAttribute('data-status')]);

  it('appear and update for Claude Code, Codex and OpenCode sessions, with their own status icons', async () => {
    const { client } = await scripted();
    act(() => client.tick(3_000)); // all three start
    expect(rows('a1')).toEqual([['explore-repo', 'done'], ['test-runner', 'run'], ['lint-fixer', 'run']]); // selected
    await userEvent.click(cardMain('a2')); // folded cards list their sub-agents when selected
    expect(rows('a2')).toEqual([['schema-check', 'run']]);
    expect(card('a2').querySelector('[data-sub-agent] [data-status]')).toHaveAccessibleName('Running');
    await userEvent.click(cardMain('a6'));
    expect(rows('a6')).toEqual([['docs-scan', 'run']]);

    act(() => client.tick(10_000)); // all three stop
    expect(rows('a6')).toEqual([['docs-scan', 'done']]);
    expect(card('a6').querySelector('[data-sub-agent] [data-status]')).toHaveAccessibleName('Done');
    await userEvent.click(cardMain('a2'));
    expect(rows('a2')).toEqual([['schema-check', 'done']]);
    await userEvent.click(cardMain('a1'));
    expect(rows('a1')).toEqual([['explore-repo', 'done'], ['test-runner', 'run'], ['lint-fixer', 'done']]);
  });

  it('running-lead rule: an idle or done lead shows run while a sub-agent runs, and settles after', async () => {
    const { client } = await scripted();
    expect(glyph('a2')).toHaveAccessibleName('Idle');
    expect(glyph('a6')).toHaveAccessibleName('Done');
    act(() => client.tick(3_000));
    expect(glyph('a2')).toHaveAccessibleName('Running');
    expect(glyph('a6')).toHaveAccessibleName('Running');
    expect(within(card('a2')).getByText('1 sub-agent · 1 running')).toBeInTheDocument();
    act(() => client.tick(10_000));
    expect(glyph('a2')).toHaveAccessibleName('Idle');
    expect(glyph('a6')).toHaveAccessibleName('Done');
  });

  it('a collapsed project rolls up as run while only a sub-agent runs', async () => {
    const { client } = await scripted();
    await userEvent.click(row(/dotfiles/)); // a6 done -> done roll-up
    expect(row(/dotfiles/).querySelector('[data-rollup="done"]')).not.toBeNull();
    act(() => client.tick(3_000));
    expect(row(/dotfiles/).querySelector('[data-rollup="run"]')).not.toBeNull();
  });
});

describe('session cards', () => {
  it('perm card: Allow / Always / Deny are visible without hover and call the client', async () => {
    const { client } = await ready();
    const resolve = vi.spyOn(client, 'resolvePermission');
    const group = within(card('a3')).getByRole('group', { name: /^Permission/ });
    expect(within(group).getAllByRole('button').map((b) => b.textContent)).toEqual(['Allow', 'Always', 'Deny']);
    expect(group.className).not.toMatch(/hidden|group-hover/);
    await userEvent.click(within(group).getByRole('button', { name: 'Always' }));
    expect(resolve).toHaveBeenCalledWith('a3', 'always');
    expect(getSelectedSessionId()).toBe('a3'); // the terminal gets focus too, so the user sees the answer land
    await waitFor(() => expect(within(card('a3')).queryByRole('group', { name: /^Permission/ })).toBeNull());
    expect(within(card('a1')).queryByRole('group', { name: /^Permission/ })).toBeNull();
  });

  it('Always reads Auto when it switches to auto mode, and is hidden when the prompt has no such option', async () => {
    const { client } = await ready();
    const resolve = vi.spyOn(client, 'resolvePermission').mockResolvedValue();
    const labels = () =>
      within(within(card('a3')).getByRole('group', { name: /^Permission/ }))
        .getAllByRole('button')
        .map((b) => b.textContent);
    act(() => client.setStatus('a3', 'perm', { tool: 'Bash', summary: 'ls', always: 'auto_mode' }));
    expect(labels()).toEqual(['Allow', 'Auto', 'Deny']);
    await userEvent.click(within(card('a3')).getByRole('button', { name: 'Auto' }));
    expect(resolve).toHaveBeenCalledWith('a3', 'always');
    act(() => client.setStatus('a3', 'perm', { tool: 'Bash', summary: 'ls', always: 'unavailable' }));
    expect(labels()).toEqual(['Allow', 'Deny']);
    act(() => client.setStatus('a3', 'perm', { tool: 'Bash', summary: 'ls', always: 'remember' }));
    expect(labels()).toEqual(['Allow', 'Always', 'Deny']);
  });

  it('each button sends its own decision for its own session', async () => {
    const { client } = await ready();
    const resolve = vi.spyOn(client, 'resolvePermission').mockResolvedValue();
    await userEvent.click(within(card('a3')).getByRole('button', { name: 'Allow' }));
    await userEvent.click(within(card('a3')).getByRole('button', { name: 'Deny' }));
    expect(resolve.mock.calls).toEqual([
      ['a3', 'allow'],
      ['a3', 'deny'],
    ]);
  });

  it('disables the three buttons while the daemon answers, then enables them again', async () => {
    const { client } = await ready();
    let finish: () => void = () => {};
    const resolve = vi.spyOn(client, 'resolvePermission').mockReturnValue(new Promise<void>((r) => (finish = r)));
    const buttons = () => within(card('a3')).getAllByRole('button', { name: /^(Allow|Always|Deny)$/ });
    await userEvent.click(buttons()[0]!);
    await waitFor(() => buttons().forEach((b) => expect(b).toBeDisabled()));
    await waitFor(() => expect(cardMain('a3')).toHaveFocus()); // focus left the button that went disabled
    await userEvent.click(buttons()[0]!); // a second click while in flight is ignored
    expect(resolve).toHaveBeenCalledTimes(1);
    finish();
    await waitFor(() => buttons().forEach((b) => expect(b).toBeEnabled()));
  });

  it('says so when the daemon could not answer the prompt (e.g. Always with no such option)', async () => {
    const { client } = await ready();
    vi.spyOn(client, 'resolvePermission').mockRejectedValue(new Error('the permission prompt has no option for this decision'));
    await userEvent.click(within(card('a3')).getByRole('button', { name: 'Always' }));
    expect(await screen.findByRole('status')).toHaveTextContent('has no option for this decision');
    await waitFor(() => expect(within(card('a3')).getByRole('button', { name: 'Always' })).toBeEnabled());
  });

  it('keyboard focus returns to the card after Allow, Retry and rename (the control it was on goes away)', async () => {
    await ready();
    await userEvent.click(within(card('a3')).getByRole('button', { name: 'Allow' }));
    await waitFor(() => expect(cardMain('a3')).toHaveFocus());
    await userEvent.click(within(card('a4')).getByRole('button', { name: /Retry/ }));
    await waitFor(() => expect(cardMain('a4')).toHaveFocus());

    fireEvent.contextMenu(card('a2'));
    await userEvent.click(screen.getByRole('menuitem', { name: /Rename/ }));
    await userEvent.keyboard('{Escape}');
    await waitFor(() => expect(cardMain('a2')).toHaveFocus());
    fireEvent.contextMenu(row(/dotfiles/));
    await userEvent.click(screen.getByRole('menuitem', { name: /Rename/ }));
    await userEvent.keyboard('{Enter}');
    await waitFor(() => expect(row(/dotfiles/)).toHaveFocus());
  });

  it('perm buttons wrap instead of overflowing a narrow sidebar', async () => {
    await ready();
    expect(within(card('a3')).getByRole('group', { name: /^Permission/ }).className).toMatch(/\bflex-wrap\b/);
  });

  it('fail card: Retry swaps with the time in the same row and restarts the session', async () => {
    const { client } = await ready();
    const retry = vi.spyOn(client, 'retrySession');
    const a4 = within(card('a4'));
    const time = a4.getByText('14m');
    expect(time.className).toMatch(/group-hover:hidden/);
    const btn = a4.getByRole('button', { name: /Retry/ });
    expect(btn.className).toMatch(/group-hover:inline-flex/);
    expect(btn.parentElement).toBe(time.parentElement); // same row: no layout shift
    await userEvent.click(btn);
    expect(retry).toHaveBeenCalledWith('a4');
    await waitFor(() => expect(within(card('a4')).queryByRole('button', { name: /Retry/ })).toBeNull());
  });

  it('folds sub-agents: listed for the selected session or one that needs you, else one line', async () => {
    await ready();
    expect(within(card('a1')).getByText('explore-repo')).toBeInTheDocument(); // selected
    expect(within(card('a3')).getByText('e2e-runner')).toBeInTheDocument(); // needs you
    await userEvent.click(cardMain('a2'));
    expect(within(card('a1')).queryByText('explore-repo')).toBeNull();
    expect(within(card('a1')).getByText('2 sub-agents · 1 running')).toBeInTheDocument();
  });

  it('unread dot only while the session is not selected; selecting reads it', async () => {
    const { client } = await ready();
    const read = vi.spyOn(client, 'markRead');
    expect(within(card('a6')).getByRole('img', { name: 'Unread' })).toBeInTheDocument();
    await userEvent.click(cardMain('a6'));
    expect(within(card('a6')).queryByRole('img', { name: 'Unread' })).toBeNull();
    expect(read).toHaveBeenCalledWith('a6');
  });

  it('time labels hold still under the pointer and catch up once it leaves', async () => {
    await ready();
    const now = Date.now();
    vi.spyOn(Date, 'now').mockReturnValue(now + 10 * 60_000); // ten minutes later
    const nav = screen.getByRole('navigation', { name: 'Projects' });
    await userEvent.hover(nav);
    await new Promise((r) => setTimeout(r, 1300)); // one clock tick
    expect(within(card('a1')).getByText('2m')).toBeInTheDocument();
    await userEvent.unhover(nav);
    await waitFor(() => expect(within(card('a1')).getByText('12m')).toBeInTheDocument(), { timeout: 2500 });
  });

  it('is draggable for split (carries the session id)', async () => {
    await ready();
    const data: Record<string, string> = {};
    fireEvent.dragStart(card('a2'), { dataTransfer: { setData: (k: string, v: string) => (data[k] = v), effectAllowed: '' } });
    expect(data['text/lotse-session']).toBe('a2');
  });
});

describe('keyboard', () => {
  it('↑/↓ moves the selection and focus between sessions, across projects, and stops at the ends', async () => {
    await ready();
    cardMain('a1').focus();
    await userEvent.keyboard('{ArrowDown}');
    expect(getSelectedSessionId()).toBe('a2');
    expect(cardMain('a2')).toHaveFocus();
    await userEvent.keyboard('{ArrowDown}');
    expect(getSelectedSessionId()).toBe('a3'); // next project
    await userEvent.keyboard('{ArrowUp}{ArrowUp}{ArrowUp}');
    expect(getSelectedSessionId()).toBe('a1'); // clamped at the top
    cardMain('a7').focus();
    await userEvent.keyboard('{ArrowDown}');
    expect(getSelectedSessionId()).toBe('a7'); // clamped at the bottom
  });

  it('skips sessions of collapsed projects', async () => {
    await ready();
    await userEvent.click(row(/praxis-web/));
    cardMain('a2').focus();
    await userEvent.keyboard('{ArrowDown}');
    expect(getSelectedSessionId()).toBe('a5');
  });

  it('Enter opens the focused session', async () => {
    await ready();
    cardMain('a4').focus();
    await userEvent.keyboard('{Enter}');
    expect(getSelectedSessionId()).toBe('a4');
  });

  it('↑/↓ from a focused project row steps to the session after / before the row, not from the selection', async () => {
    await ready();
    row(/praxis-web/).focus();
    await userEvent.keyboard('{ArrowDown}');
    expect(getSelectedSessionId()).toBe('a3'); // first session under the row (selection was a1)
    row(/infra-terraform/).focus();
    await userEvent.keyboard('{ArrowUp}');
    expect(getSelectedSessionId()).toBe('a4'); // last session above the row
  });

  it('→ / ← expand and collapse the focused project', async () => {
    await ready();
    row(/billing-api/).focus();
    await userEvent.keyboard('{ArrowLeft}');
    expect(row(/billing-api/)).toHaveAttribute('aria-expanded', 'false');
    await userEvent.keyboard('{ArrowRight}');
    expect(row(/billing-api/)).toHaveAttribute('aria-expanded', 'true');
  });
});

describe('need-you filter', () => {
  async function withBar() {
    const view = mount(
      <>
        <Sidebar />
        <StatusBar />
      </>,
    );
    await screen.findByText('billing-api');
    return view;
  }

  it('shows only sessions that need you, jumps to the most urgent, and Show all restores', async () => {
    await withBar();
    await userEvent.click(screen.getByRole('button', { name: '▲ 2 need you' }));
    expect(screen.getByText('Needs you (2)')).toBeInTheDocument();
    expect([...document.querySelectorAll('[data-sid]')].map((e) => e.getAttribute('data-sid'))).toEqual(['a3', 'a4']);
    expect(screen.queryByText('billing-api')).toBeNull();
    expect(getSelectedSessionId()).toBe('a3'); // perm outranks fail
    expect(screen.getByRole('button', { name: /Showing 2 that need you/ })).toHaveAttribute('aria-pressed', 'true');

    await userEvent.click(screen.getByRole('button', { name: 'Show all' }));
    expect(document.querySelectorAll('[data-sid]')).toHaveLength(7);
    expect(screen.getByRole('button', { name: '▲ 2 need you' })).toHaveAttribute('aria-pressed', 'false');
  });

  it('turns itself off when nothing needs you any more', async () => {
    const { client } = await withBar();
    await userEvent.click(screen.getByRole('button', { name: '▲ 2 need you' }));
    await act(() => client.resolvePermission('a3', 'allow'));
    await act(() => client.retrySession('a4'));
    await waitFor(() => expect(document.querySelectorAll('[data-sid]')).toHaveLength(7));
    expect(screen.getByRole('button', { name: '▲ 0 need you' })).toBeDisabled();
  });
});

describe('truncation', () => {
  it('agent · branch line: only the branch truncates; the agent label and ↑/± counts never shrink', async () => {
    await ready();
    const line = card('a3').querySelector<HTMLElement>('[data-line="git"]');
    const agent = line?.querySelector<HTMLElement>('[data-part="agent"]');
    const branch = line?.querySelector<HTMLElement>('[data-part="branch"]');
    expect(agent?.className).toMatch(/\bshrink-0\b/);
    expect(agent?.className).not.toMatch(/truncate|shrink-\[/);
    expect(branch?.className).toMatch(/\btruncate\b/);
    expect(branch?.className).toMatch(/\bmin-w-0\b/);
    expect(branch).toHaveAttribute('title', 'lotse/patients-signals');
    for (const part of ['ahead', 'dirty']) {
      expect(line?.querySelector(`[data-part="${part}"]`)?.className).toMatch(/\bshrink-0\b/);
    }
  });

  const longName = 'a-very-long-project-name-that-would-push-the-sidebar-wider-than-its-limit';
  const longTitle = 'An extremely long session title that must be cut with an ellipsis and never widen the sidebar';
  const longBranch = 'lotse/an-extremely-long-branch-name-for-a-worktree-session-that-goes-on-and-on';

  it('cuts long project, title, branch and sub-agent names with "…" and keeps the full value in the tooltip', async () => {
    const { client } = await ready();
    await act(async () => {
      const p = await client.addProject({ path: `/x/${longName}`, name: longName, color: null, icon: null });
      const s = await client.createSession({
        project_id: p.id,
        agent: 'claude_code',
        command: null,
        title: longTitle,
        worktree: { branch: longBranch },
        first_prompt: null,
      });
      client.setStatus(s.id, 'perm', { tool: 'Bash', summary: 'ls' });
    });
    const name = await screen.findByText(longName);
    expect(name).toHaveAttribute('title', longName);
    expect(name.className).toMatch(/\btruncate\b/);
    expect(name.className).toMatch(/\bmin-w-0\b/);

    const title = screen.getByText(longTitle);
    expect(title).toHaveAttribute('title', longTitle);
    expect(title.className).toMatch(/\btruncate\b/);
    expect(title.className).toMatch(/\bmin-w-0\b/);

    const branch = screen.getByText(longBranch.replace('lotse/', ''));
    expect(branch).toHaveAttribute('title', longBranch);
    expect(branch.className).toMatch(/\btruncate\b/);
    expect(branch.className).toMatch(/\bmin-w-0\b/);

    // the list never scrolls sideways and cards never exceed it
    expect(screen.getByRole('navigation', { name: 'Projects' }).className).toMatch(/overflow-x-hidden/);
    expect(screen.getByText('e2e-runner').className).toMatch(/\btruncate\b/);
  });
});

describe('context menus', () => {
  it('project menu: items, Notifications submenu toggles mute, Esc closes', async () => {
    const { client } = await ready();
    fireEvent.contextMenu(row(/billing-api/));
    const menu = screen.getByRole('menu', { name: 'billing-api actions' });
    const labels = within(menu).getAllByRole('menuitem').map((i) => i.textContent);
    expect(labels).toEqual([
      expect.stringContaining('New Session…'),
      expect.stringContaining('Notifications'),
      expect.stringContaining('Change Icon & Color…'),
      expect.stringContaining('Move to Workspace'),
      expect.stringContaining('Reveal in Finder'),
      expect.stringContaining('Copy Path'),
      expect.stringContaining('Rename…'),
      expect.stringContaining('Remove from Lotse…'),
    ]);
    await userEvent.hover(within(menu).getByRole('menuitem', { name: /Notifications/ }));
    const sub = screen.getByRole('menu', { name: 'Notifications' });
    expect(within(sub).getByRole('menuitemcheckbox', { name: /Use global defaults/ })).toHaveAttribute('aria-checked', 'true');
    expect(within(sub).getByRole('menuitemcheckbox', { name: /Failed/ })).toHaveAttribute('aria-disabled', 'true');
    await userEvent.click(within(sub).getByRole('menuitemcheckbox', { name: /Mute this project/ }));
    await waitFor(() => expect(client.listProjects().then((p) => p[0]?.notifications)).resolves.toEqual({ mode: 'mute' }));
    expect(await screen.findByRole('img', { name: 'Notifications muted' })).toBeInTheDocument();
    expect(within(screen.getByRole('menu', { name: 'Notifications' })).getByRole('menuitemcheckbox', { name: /Mute this project/ })).toHaveAttribute('aria-checked', 'true');
    await userEvent.keyboard('{Escape}');
    await userEvent.keyboard('{Escape}');
    expect(screen.queryByRole('menu')).toBeNull();
  });

  it('project menu › New Session… starts the terminal in that project even when another one is current', async () => {
    const { client } = await ready();
    const create = vi.spyOn(client, 'createSession');
    act(() => void selectSession('a3')); // praxis-web
    fireEvent.contextMenu(row(/billing-api/));
    await userEvent.click(screen.getByRole('menuitem', { name: /New Session/ }));
    await userEvent.click(screen.getByRole('menuitem', { name: /^New Terminal/ }));
    expect(create).toHaveBeenCalledWith(expect.objectContaining({ project_id: 'p1', agent: 'terminal' }));
  });

  it('project menu: icon & colour and New Session carry the project id; Reveal in Finder is app-only', async () => {
    await ready();
    const style = vi.fn();
    const fresh = vi.fn();
    const offs = [onUiCommand('openProjectStyle', style), onUiCommand('openNewSession', fresh)];
    fireEvent.contextMenu(row(/billing-api/));
    // in the browser there is no Finder to ask
    expect(screen.getByRole('menuitem', { name: /Reveal in Finder/ })).toHaveAttribute('aria-disabled', 'true');
    await userEvent.click(screen.getByRole('menuitem', { name: /Change Icon & Color/ }));
    expect(style).toHaveBeenCalledWith('p1');
    fireEvent.contextMenu(row(/billing-api/));
    await userEvent.click(screen.getByRole('menuitem', { name: /New Session/ }));
    // New Session… opens the new-session dropdown for the project, in place of the menu
    expect(screen.queryByRole('menu', { name: 'billing-api actions' })).toBeNull();
    await userEvent.click(within(screen.getByRole('menu', { name: 'New session' })).getByRole('menuitem', { name: /Worktree or other project…/ }));
    expect(fresh).toHaveBeenCalledWith('p1');
    offs.forEach((off) => off());

    // with a native bridge the item reveals the project's folder
    const reveal = vi.fn().mockResolvedValue(undefined);
    setHostBridge({ ...hostBridge(), revealInFinder: reveal });
    fireEvent.contextMenu(row(/billing-api/));
    await userEvent.click(screen.getByRole('menuitem', { name: /Reveal in Finder/ }));
    expect(reveal).toHaveBeenCalledWith(expect.stringMatching(/^\//));
    setHostBridge(null);
  });

  it('Retry on a card whose process still runs shows the daemon\'s message instead of failing silently', async () => {
    const { client } = await ready();
    vi.spyOn(client, 'retrySession').mockRejectedValue(new Error('the process is still running; Retry re-runs a command that has exited'));
    await userEvent.click(within(card('a4')).getByRole('button', { name: /Retry/ }));
    expect(await screen.findByRole('status')).toHaveTextContent('still running');
  });

  it('custom notifications start from the global toggles and can be changed per event', async () => {
    const { client } = await ready();
    fireEvent.contextMenu(row(/billing-api/));
    await userEvent.hover(screen.getByRole('menuitem', { name: /Notifications/ }));
    await userEvent.click(screen.getByRole('menuitemcheckbox', { name: /Custom for this project/ }));
    await userEvent.click(await screen.findByRole('menuitemcheckbox', { name: /Sub-agent done/ }));
    const p = (await client.listProjects())[0];
    expect(p?.notifications).toEqual({ mode: 'custom', perm: true, fail: true, done: true, sub_agent_done: true });
    expect(await screen.findByRole('img', { name: 'Custom notifications' })).toBeInTheDocument();
  });

  it('choosing "Custom for this project" again keeps the per-event toggles', async () => {
    const { client } = await ready();
    await client.updateProject('p1', { notifications: { mode: 'custom', perm: false, fail: true, done: false, sub_agent_done: true } });
    fireEvent.contextMenu(row(/billing-api/));
    await userEvent.hover(screen.getByRole('menuitem', { name: /Notifications/ }));
    await userEvent.click(await screen.findByRole('menuitemcheckbox', { name: /Custom for this project/ }));
    expect((await client.listProjects())[0]?.notifications).toEqual({ mode: 'custom', perm: false, fail: true, done: false, sub_agent_done: true });
  });

  it('Copy Path writes the project path to the clipboard', async () => {
    await ready();
    const writeText = vi.fn().mockResolvedValue(undefined);
    Object.defineProperty(navigator, 'clipboard', { value: { writeText }, configurable: true });
    fireEvent.contextMenu(row(/praxis-web/));
    await userEvent.click(screen.getByRole('menuitem', { name: /Copy Path/ }));
    expect(writeText).toHaveBeenCalledWith('/Users/naim/Repos/praxis-web');
  });

  it('Rename edits the project name in place', async () => {
    const { client } = await ready();
    fireEvent.contextMenu(row(/dotfiles/));
    await userEvent.click(screen.getByRole('menuitem', { name: /Rename/ }));
    const input = screen.getByRole('textbox', { name: 'Project name' });
    expect(input).toHaveFocus();
    await userEvent.keyboard('{Control>}a{/Control}dots{Enter}');
    await waitFor(() => expect(client.listProjects().then((p) => p[3]?.name)).resolves.toBe('dots'));
    expect(await screen.findByText('dots')).toBeInTheDocument();
  });

  it('Remove asks first, then removes the project and its sessions', async () => {
    const { client } = await ready();
    fireEvent.contextMenu(row(/dotfiles/));
    await userEvent.click(screen.getByRole('menuitem', { name: /Remove from Lotse/ }));
    const dialog = screen.getByRole('alertdialog');
    await userEvent.click(within(dialog).getByRole('button', { name: 'Cancel' }));
    expect(screen.getByText('dotfiles')).toBeInTheDocument();
    fireEvent.contextMenu(row(/dotfiles/));
    await userEvent.click(screen.getByRole('menuitem', { name: /Remove from Lotse/ }));
    await userEvent.click(within(screen.getByRole('alertdialog')).getByRole('button', { name: 'Remove' }));
    await waitFor(() => expect(screen.queryByText('dotfiles')).toBeNull());
    expect(await client.listProjects()).toHaveLength(4);
    expect(document.querySelector('[data-sid="a6"]')).toBeNull();
  });

  it('Show Transcript and Show Changes: on for a Codex session in a git folder, and they raise their UI commands', async () => {
    await ready();
    const transcript = vi.fn();
    const changes = vi.fn();
    const offs = [onUiCommand('showTranscript', transcript), onUiCommand('showChanges', changes)];
    fireEvent.contextMenu(card('a2'));
    let menu = screen.getByRole('menu', { name: 'Flyway migration V42 actions' });
    expect(within(menu).getByRole('menuitem', { name: /Show Transcript/ })).not.toHaveAttribute('aria-disabled', 'true');
    expect(within(menu).getByRole('menuitem', { name: /Show Changes/ })).not.toHaveAttribute('aria-disabled', 'true');
    await userEvent.click(within(menu).getByRole('menuitem', { name: /Show Transcript/ }));
    expect(transcript).toHaveBeenCalledWith('a2');
    fireEvent.contextMenu(card('a2'));
    menu = screen.getByRole('menu', { name: 'Flyway migration V42 actions' });
    await userEvent.click(within(menu).getByRole('menuitem', { name: /Show Changes/ }));
    expect(changes).toHaveBeenCalledWith('a2');
    offs.forEach((off) => off());
  });

  it('Show Transcript is off with a reason for OpenCode and Terminal; Show Changes is off without git', async () => {
    await ready();
    // a6 is OpenCode in a git folder
    fireEvent.contextMenu(card('a6'));
    let menu = screen.getByRole('menu', { name: /actions$/ });
    const t = within(menu).getByRole('menuitem', { name: /Show Transcript/ });
    expect(t).toHaveAttribute('aria-disabled', 'true');
    expect(t).toHaveAttribute('title', 'Not available for OpenCode');
    expect(within(menu).getByRole('menuitem', { name: /Show Changes/ })).not.toHaveAttribute('aria-disabled', 'true');
    await userEvent.keyboard('{Escape}');
    // a7 is a Terminal in the notes folder: no transcript, no git
    fireEvent.contextMenu(card('a7'));
    menu = screen.getByRole('menu', { name: /actions$/ });
    expect(within(menu).getByRole('menuitem', { name: /Show Transcript/ })).toHaveAttribute('title', 'Not available for Terminal');
    const c = within(menu).getByRole('menuitem', { name: /Show Changes/ });
    expect(c).toHaveAttribute('aria-disabled', 'true');
    expect(c).toHaveAttribute('title', expect.stringMatching(/^No git/));
    expect(c).toHaveTextContent('No git');
  });

  it('session menu: Open, Stop closes an idle session at once', async () => {
    const { client } = await ready();
    fireEvent.contextMenu(card('a2'));
    const menu = screen.getByRole('menu', { name: 'Flyway migration V42 actions' });
    expect(within(menu).getByRole('menuitem', { name: /Split Right/ })).not.toHaveAttribute('aria-disabled', 'true');
    await userEvent.click(within(menu).getByRole('menuitem', { name: /^Open(?! in)/ }));
    expect(getSelectedSessionId()).toBe('a2');
    // Split Right asks the panes (UI command bus) for this session
    const split = vi.fn();
    const off = onUiCommand('splitSession', split);
    fireEvent.contextMenu(card('a2'));
    await userEvent.click(screen.getByRole('menuitem', { name: /Split Right/ }));
    expect(split).toHaveBeenCalledWith('a2');
    off();
    // Split Down is its own command, so the panes know which way to split
    const splitDown = vi.fn();
    const offDown = onUiCommand('splitSessionDown', splitDown);
    fireEvent.contextMenu(card('a2'));
    expect(screen.getByRole('menuitem', { name: /Split Down/ })).toHaveTextContent('⌥⇧-click');
    await userEvent.click(screen.getByRole('menuitem', { name: /Split Down/ }));
    expect(splitDown).toHaveBeenCalledWith('a2');
    offDown();
    fireEvent.contextMenu(card('a2'));
    await userEvent.click(screen.getByRole('menuitem', { name: /Stop/ }));
    await waitFor(() => expect(document.querySelector('[data-sid="a2"]')).toBeNull());
    expect(await client.listSessions()).toHaveLength(6);
  });

  it('Stop on a working session asks first', async () => {
    await ready();
    fireEvent.contextMenu(card('a5'));
    await userEvent.click(screen.getByRole('menuitem', { name: /Stop/ }));
    expect(screen.getByRole('alertdialog')).toHaveTextContent('Stop RDS Postgres 17?');
    await userEvent.keyboard('{Escape}');
    expect(card('a5')).toBeInTheDocument();
  });

  it('menus are keyboard operable: ↓ moves, Enter activates', async () => {
    await ready();
    fireEvent.contextMenu(card('a2'));
    expect(screen.getByRole('menuitem', { name: /^Open(?! in)/ })).toHaveFocus();
    await userEvent.keyboard('{ArrowDown}'); // Split Right
    await userEvent.keyboard('{ArrowDown}'); // Split Down
    expect(screen.getByRole('menuitem', { name: /Split Down/ })).toHaveFocus();
    await userEvent.keyboard('{ArrowDown}');
    expect(screen.getByRole('menuitem', { name: /Rename/ })).toHaveFocus();
    await userEvent.keyboard('{Enter}');
    expect(screen.getByRole('textbox', { name: 'Session title' })).toBeInTheDocument();
  });
});

describe('workspaces', () => {
  const wsRow = (name: string) => screen.getByRole('button', { name: new RegExp(`^${name}`) });
  const inWorkspace = (id: string) => within(document.querySelector<HTMLElement>(`[data-workspace="${id}"]`)!);

  /** Two workspaces: praxis-web (a perm and a failed session) moves into "Client work". */
  async function withClientWork() {
    const view = await ready();
    const ws = await view.client.createWorkspace('Client work');
    await view.client.moveProject('p2', ws.id);
    await screen.findByRole('button', { name: /^Client work/ });
    return { ...view, ws };
  }

  it('with one workspace there are no headers: the tree is the mock\'s', async () => {
    await ready();
    expect(document.querySelector('[data-wid]')).toBeNull();
    expect(document.querySelector('[data-workspace]')).toBeNull();
    expect(screen.queryByText('Default')).toBeNull();
  });

  it('with several, headers group the projects, in daemon order, and an empty one says so', async () => {
    const { client, ws } = await withClientWork();
    await client.createWorkspace('Home');
    await screen.findByRole('button', { name: /^Home/ });
    expect([...document.querySelectorAll('[data-wid]')].map((h) => h.textContent)).toEqual(['Default', 'Client work', 'Home']);
    expect(inWorkspace('w1').getByText('billing-api')).toBeInTheDocument();
    expect(inWorkspace('w1').queryByText('praxis-web')).toBeNull();
    expect(inWorkspace(ws.id).getByText('praxis-web')).toBeInTheDocument();
    expect(document.querySelector(`[data-workspace="${ws.id}"] [data-sid="a3"]`)).not.toBeNull();
    expect(screen.getByText('No projects')).toBeInTheDocument();
  });

  it('collapsing a workspace hides its projects, rolls the most urgent state up, and is remembered', async () => {
    const { ws } = await withClientWork();
    await userEvent.click(wsRow('Client work'));
    expect(wsRow('Client work')).toHaveAttribute('aria-expanded', 'false');
    expect(screen.queryByText('praxis-web')).toBeNull();
    expect(document.querySelector('[data-sid="a3"]')).toBeNull();
    // praxis-web has a permission request (a3) and a failure (a4): ▲ and ✕ with their colours
    const header = within(wsRow('Client work'));
    expect(header.getByText('', { selector: '[data-rollup="perm"]' })).toBeInTheDocument();
    expect(wsRow('Client work').querySelector('[data-rollup="fail"]')).not.toBeNull();
    expect(JSON.parse(localStorage.getItem('lotse-collapsed') ?? '[]')).toEqual([ws.id]);
    await userEvent.click(wsRow('Client work'));
    expect(screen.getByText('praxis-web')).toBeInTheDocument();
    expect(wsRow('Client work').querySelector('[data-rollup]')).toBeNull();
  });

  it('the roll-up of a collapsed workspace is the top state: run beats done', async () => {
    const { client } = await ready();
    const ws = await client.createWorkspace('Tools');
    await client.moveProject('p4', ws.id); // dotfiles: done
    await client.moveProject('p3', ws.id); // infra-terraform: run
    await screen.findByRole('button', { name: /^Tools/ });
    await userEvent.click(wsRow('Tools'));
    expect(wsRow('Tools').querySelector('[data-rollup="run"]')).not.toBeNull();
  });

  it('arrow keys walk the visible sessions and skip a collapsed workspace', async () => {
    await withClientWork();
    await userEvent.click(wsRow('Client work')); // collapse
    cardMain('a1').focus();
    selectSession('a1');
    const last = ['a1', 'a2', 'a5', 'a6', 'a7'];
    // a1 a2 (billing-api), a5 (infra), a6 (dotfiles), a7 (notes); a3 a4 are hidden
    for (const id of last.slice(1)) {
      await userEvent.keyboard('{ArrowDown}');
      expect(getSelectedSessionId()).toBe(id);
    }
    // ← on a workspace header collapses, → expands
    wsRow('Default').focus();
    await userEvent.keyboard('{ArrowLeft}');
    expect(wsRow('Default')).toHaveAttribute('aria-expanded', 'false');
    await userEvent.keyboard('{ArrowRight}');
    expect(wsRow('Default')).toHaveAttribute('aria-expanded', 'true');
  });

  it('project menu: Move to Workspace lists the workspaces (current one checked) and moves the project', async () => {
    const { client, ws } = await withClientWork();
    fireEvent.contextMenu(row(/billing-api/));
    await userEvent.hover(screen.getByRole('menuitem', { name: /Move to Workspace/ }));
    const sub = screen.getByRole('menu', { name: 'Move to Workspace' });
    expect(within(sub).getByRole('menuitemcheckbox', { name: 'Default' })).toHaveAttribute('aria-checked', 'true');
    expect(within(sub).getByRole('menuitemcheckbox', { name: 'Client work' })).toHaveAttribute('aria-checked', 'false');
    expect(within(sub).getByRole('menuitem', { name: /New Workspace…/ })).toBeInTheDocument();
    await userEvent.click(within(sub).getByRole('menuitemcheckbox', { name: 'Client work' }));
    await waitFor(() => expect(inWorkspace(ws.id).queryByText('billing-api')).not.toBeNull());
    expect((await client.listProjects()).find((p) => p.id === 'p1')?.workspace_id).toBe(ws.id);
  });

  it('"New Workspace…" in the project menu asks for a name, creates the workspace and moves the project there', async () => {
    const { client } = await ready();
    fireEvent.contextMenu(row(/notes/));
    await userEvent.hover(screen.getByRole('menuitem', { name: /Move to Workspace/ }));
    await userEvent.click(screen.getByRole('menuitem', { name: /New Workspace…/ }));
    const dialog = screen.getByRole('dialog', { name: 'New Workspace' });
    // a name that is taken keeps the dialog open with the daemon's reason
    await userEvent.type(within(dialog).getByRole('textbox', { name: 'Workspace name' }), 'default{Enter}');
    expect(await within(dialog).findByRole('alert')).toHaveTextContent('a workspace with that name exists');
    const input = within(dialog).getByRole('textbox', { name: 'Workspace name' });
    await userEvent.clear(input);
    await userEvent.type(input, 'Private{Enter}');
    await waitFor(() => expect(screen.queryByRole('dialog', { name: 'New Workspace' })).toBeNull());
    const ws = (await client.getSnapshot()).workspaces.find((w) => w.name === 'Private')!;
    await waitFor(() => expect(inWorkspace(ws.id).queryByText('notes')).not.toBeNull());
  });

  it('workspace menu: rename in place; Delete moves the projects to the default workspace after asking', async () => {
    const { client, ws } = await withClientWork();
    fireEvent.contextMenu(wsRow('Client work'));
    const menu = screen.getByRole('menu', { name: 'Client work actions' });
    expect(within(menu).getAllByRole('menuitem').map((i) => i.textContent)).toEqual(['New Workspace…', 'Rename…', 'Delete Workspace…']);
    await userEvent.click(within(menu).getByRole('menuitem', { name: 'Rename…' }));
    const input = screen.getByRole('textbox', { name: 'Workspace name' });
    await userEvent.clear(input);
    await userEvent.type(input, 'Clients{Enter}');
    await screen.findByRole('button', { name: /^Clients/ });
    expect((await client.getSnapshot()).workspaces.map((w) => w.name)).toEqual(['Default', 'Clients']);

    fireEvent.contextMenu(wsRow('Clients'));
    await userEvent.click(screen.getByRole('menuitem', { name: /Delete Workspace/ }));
    expect(screen.getByRole('alertdialog')).toHaveTextContent('Its 1 project move to Default');
    await userEvent.click(within(screen.getByRole('alertdialog')).getByRole('button', { name: 'Delete' }));
    await waitFor(() => expect(document.querySelector('[data-wid]')).toBeNull()); // one workspace left: no headers
    expect(screen.getByText('praxis-web')).toBeInTheDocument();
    expect((await client.listProjects()).find((p) => p.id === 'p2')?.workspace_id).toBe('w1');
    expect(ws.id).not.toBe('w1');
  });

  it('the default workspace cannot be deleted from its menu', async () => {
    await withClientWork();
    fireEvent.contextMenu(wsRow('Default'));
    const item = screen.getByRole('menuitem', { name: /Delete Workspace/ });
    expect(item).toHaveAttribute('aria-disabled', 'true');
    expect(item).toHaveAttribute('title', 'The default workspace cannot be deleted');
  });

  it('the need-you filter keeps only the workspaces with something that needs you', async () => {
    const { client } = await withClientWork();
    const sessions = await client.listSessions();
    act(() => toggleNeedFilter(sessions));
    await waitFor(() => expect(screen.queryByRole('button', { name: /^Default/ })).toBeNull());
    expect(wsRow('Client work')).toBeInTheDocument();
    expect(document.querySelector('[data-workspace] [data-sid="a3"]')).not.toBeNull();
  });
});
