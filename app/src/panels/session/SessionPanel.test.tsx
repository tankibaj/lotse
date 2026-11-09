import { act, screen, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { afterEach, describe, expect, it, vi } from 'vitest';
import { MockClient } from '../../lib/client';
import { mount } from '../../sidebar/testUtils';
import { setConfigOpener } from './configOpen';
import { SessionPanel } from './index';

afterEach(() => setConfigOpener(undefined));

async function panel(id: string, client?: MockClient) {
  const view = mount(<SessionPanel sessionId={id} />, client);
  await screen.findByTestId('session-header');
  return view;
}

describe('SessionPanel', () => {
  it('header_shows_version_and_model', async () => {
    await panel('a1');
    const head = screen.getByTestId('session-header');
    expect(head).toHaveTextContent('Claude Code');
    expect(head).toHaveTextContent('2.1.309');
    expect(head).toHaveTextContent('Running');
    expect(head).toHaveTextContent('Opus 5.5');
  });

  it('shows a Codex session without a version, and a plain terminal without version and model', async () => {
    const codex = await panel('a2');
    expect(screen.getByTestId('session-header')).toHaveTextContent('Codex');
    expect(screen.getByTestId('session-header')).toHaveTextContent('Idle · Opus 5.5');
    codex.unmount();
    await panel('a7');
    const shell = screen.getByTestId('session-header');
    expect(shell).toHaveTextContent('Terminal');
    expect(shell).not.toHaveTextContent('Opus');
  });

  it('shows the environment: machine, project, branch with ahead and dirty, worktree and detection source', async () => {
    await panel('a1');
    const env = screen.getByTestId('session-environment');
    expect(env).toHaveTextContent("Naim's MacBook Pro");
    expect(env).toHaveTextContent('billing-api');
    expect(env).toHaveTextContent('lotse/fix-rounding');
    expect(env).toHaveTextContent('↑2');
    expect(env).toHaveTextContent('±3');
    expect(env).toHaveTextContent('/Users/naim/.lotse/worktrees/billing-api/fix-rounding');
    expect(env).toHaveTextContent('Detected via Hooks');
  });

  it('says "No git" for a plain folder and names the process source', async () => {
    await panel('a7');
    const env = screen.getByTestId('session-environment');
    expect(env).toHaveTextContent('No git');
    expect(env).toHaveTextContent('Detected via Process');
  });

  it('usage_lists_input_output_and_cache_separately', async () => {
    // cumulative session totals, not the current context: named "Usage", every kind its own row, no combined total
    await panel('a1');
    const bar = await screen.findByRole('button', { name: /Usage/ });
    expect(screen.queryByRole('button', { name: /Context/ })).toBeNull();
    expect(bar).toHaveAttribute('aria-expanded', 'false');
    expect(screen.queryByText('Session cost')).toBeNull();
    await userEvent.click(bar);
    expect(bar).toHaveAttribute('aria-expanded', 'true');
    expect(screen.getByText('Input').nextSibling).toHaveTextContent('61k');
    expect(screen.getByText('Output').nextSibling).toHaveTextContent('9k');
    expect(screen.getByText('Cache read').nextSibling).toHaveTextContent('26k');
    expect(screen.getByText('Cache write').nextSibling).toHaveTextContent('0');
    expect(screen.getByText('Session cost').nextSibling).toHaveTextContent('$1.14');
    const panelText = screen.getByTestId('panel-session').textContent ?? '';
    for (const total of ['87k', '96k', '200k', '%']) expect(panelText).not.toContain(total);
  });

  it('has no usage section for a plain terminal', async () => {
    await panel('a7');
    expect(screen.queryByRole('button', { name: /Usage/ })).toBeNull();
  });

  it('lists the sub-agents with their state', async () => {
    await panel('a1');
    const subs = screen.getByTestId('session-subagents');
    expect(within(subs).getByText('explore-repo')).toBeInTheDocument();
    expect(within(subs).getByText('test-runner')).toBeInTheDocument();
    expect(within(subs).getByLabelText('Done')).toBeInTheDocument();
    expect(within(subs).getByLabelText('Running')).toBeInTheDocument();
  });

  it('has no sub-agents section for a session without sub-agents', async () => {
    await panel('a2');
    expect(screen.queryByTestId('session-subagents')).toBeNull();
  });

  it('stop_task_confirms_then_calls_client', async () => {
    const { client } = await panel('a1');
    const stop = vi.spyOn(client, 'stopTask');
    expect(await screen.findByText('npm run dev')).toBeInTheDocument();

    await userEvent.click(screen.getByRole('button', { name: 'Stop task npm run dev' }));
    const dialog = screen.getByRole('alertdialog');
    expect(dialog).toHaveTextContent('npm run dev');
    expect(stop).not.toHaveBeenCalled(); // asking is not stopping

    await userEvent.click(within(dialog).getByRole('button', { name: 'Cancel' }));
    expect(screen.queryByRole('alertdialog')).toBeNull();
    expect(stop).not.toHaveBeenCalled();
    expect(screen.getByText('npm run dev')).toBeInTheDocument();

    await userEvent.click(screen.getByRole('button', { name: 'Stop task npm run dev' }));
    await userEvent.click(within(screen.getByRole('alertdialog')).getByRole('button', { name: 'Stop' }));
    expect(stop).toHaveBeenCalledExactlyOnceWith('a1', 4711);
    await vi.waitFor(() => expect(screen.queryByText('npm run dev')).toBeNull());
  });

  it('has no background tasks section for a session without tasks', async () => {
    await panel('a2');
    expect(screen.queryByText('Background tasks')).toBeNull();
  });

  it('connectors_empty_state', async () => {
    const client = new MockClient({ script: false });
    vi.spyOn(client, 'listConnectors').mockResolvedValue([]);
    await panel('a1', client);
    expect(await screen.findByText('No MCP servers configured.')).toBeInTheDocument();
    expect(screen.queryByRole('button', { name: /Manage/ })).toBeNull();
  });

  it('lists configured servers without a live status until Settings › External allows the check', async () => {
    await panel('a1');
    const list = await screen.findByTestId('session-connectors');
    expect(within(list).getByText('Agent browser')).toBeInTheDocument();
    expect(within(list).getByText('Jfrog')).toBeInTheDocument();
    expect(within(list).getByText('Mcp atlassian')).toBeInTheDocument();
    expect(within(list).queryByText('Playwright')).toBeNull(); // first three, the rest behind "Show 2 more"
    expect(within(list).queryByText(/tools$/)).toBeNull(); // configured, not checked
    await userEvent.click(within(list).getByRole('button', { name: 'Show 2 more' }));
    expect(within(list).getByText('Mermaid Chart')).toBeInTheDocument();
    await userEvent.click(within(list).getByRole('button', { name: 'Show less' }));
    expect(within(list).queryByText('Mermaid Chart')).toBeNull();
  });

  it('shows live status, tool counts, and Retry on a failed server only with mcp_live on', async () => {
    const client = new MockClient({ script: false });
    const s = await client.getSettings();
    await client.updateSettings({ ...s, external: { ...s.external, mcp_live: true } });
    await panel('a2', client); // Codex: Playwright connected, GitHub failed
    const list = await screen.findByTestId('session-connectors');
    expect(await within(list).findByText('21 tools')).toBeInTheDocument();
    const check = vi.spyOn(client, 'checkConnectors');
    await userEvent.click(within(list).getByRole('button', { name: 'Retry' }));
    expect(check).toHaveBeenCalledExactlyOnceWith('a2');
  });

  it('shows "Sign in…" for a server that needs auth', async () => {
    const client = new MockClient({ script: false });
    const s = await client.getSettings();
    await client.updateSettings({ ...s, external: { ...s.external, mcp_live: true } });
    await panel('a1', client);
    await userEvent.click(await screen.findByRole('button', { name: 'Show 2 more' }));
    expect(screen.getByText('Sign in…')).toBeInTheDocument();
  });

  it('offers no Retry while the live check is off, and drops it when it is switched off', async () => {
    const client = new MockClient({ script: false });
    const s = await client.getSettings();
    await client.updateSettings({ ...s, external: { ...s.external, mcp_live: true } });
    await panel('a2', client);
    const retry = await screen.findByRole('button', { name: 'Retry' });
    await act(() => client.updateSettings({ ...s, external: { ...s.external, mcp_live: false } }));
    await vi.waitFor(() => expect(retry).not.toBeInTheDocument());
    expect(screen.queryByText('Failed')).toBeNull(); // configured again: no live status
  });

  it('Manage opens the config file the daemon listed', async () => {
    const open = vi.fn().mockResolvedValue(undefined);
    setConfigOpener(open);
    await panel('a2');
    await userEvent.click(await screen.findByRole('button', { name: 'Manage' }));
    expect(open).toHaveBeenCalledExactlyOnceWith('a2', '~/.codex/config.toml');
  });

  it('Manage is disabled outside the app (no native opener)', async () => {
    setConfigOpener(null);
    await panel('a2');
    expect(await screen.findByRole('button', { name: 'Manage' })).toBeDisabled();
  });

  it('shows nothing for no selected session', async () => {
    mount(<SessionPanel sessionId={null} />);
    expect(await screen.findByText('No session selected.')).toBeInTheDocument();
  });
});
