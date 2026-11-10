import { act, screen, waitFor, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { afterEach, describe, expect, it, vi } from 'vitest';
import { emitUiCommand } from '../lib/ui/commands';
import { mountWp5 } from '../sheets/harness';
import { setAutoAllowBridge, validateRule } from './autoallow';
import type { AutoAllowBridge, AutoAllowConfig, AutoAllowRule } from './autoallow';

afterEach(() => {
  setAutoAllowBridge(undefined);
  localStorage.clear();
});

const bash = (command: string, extra: Partial<AutoAllowRule> = {}): AutoAllowRule => ({
  id: 'r1',
  project_id: 'p1',
  tool: 'Bash',
  command,
  prefix: false,
  allow_destructive: false,
  ...extra,
});

describe('validateRule (mirrors the daemon; the daemon decides)', () => {
  it('accepts plain commands and project-relative directories', () => {
    expect(validateRule(bash('git status'))).toBeNull();
    expect(validateRule(bash('git status --porcelain', { prefix: true }))).toBeNull();
    expect(validateRule({ id: 'r', project_id: 'p1', tool: 'Read', dir: 'src', prefix: false, allow_destructive: false })).toBeNull();
    expect(validateRule({ id: 'r', project_id: 'p1', tool: 'Read', dir: '', prefix: false, allow_destructive: false })).toBeNull();
  });

  it('rejects chained, quoted, expanded, multi-line, traversing and non-ASCII commands', () => {
    for (const bad of [
      '', 'ls; id', 'ls && id', 'ls | wc', 'ls & ', 'echo $HOME', 'echo `id`', 'echo $(id)', 'ls > x', 'ls < x',
      'ls\nid', 'ls\tid', 'ls  -l', ' ls', 'ls ', "ls 'x'", 'ls "x"', 'ls \\x', 'ls *', 'ls ~', 'ls ../..', 'ls é', 'ls -l',
      'ls​', 'ｌs', 'a'.repeat(1001),
    ]) {
      expect(validateRule(bash(bad)), JSON.stringify(bad)).not.toBeNull();
    }
    expect(validateRule(bash('FOO=1 ls', { prefix: true }))).not.toBeNull();
  });

  it('rejects destructive commands unless explicit, and bad ids, tools and directories', () => {
    for (const cmd of ['rm -rf build', 'sudo ls', 'curl example.com', 'git push', 'git reset --hard', '/bin/rm x', 'find . -delete']) {
      expect(validateRule(bash(cmd)), cmd).not.toBeNull();
      expect(validateRule(bash(cmd, { allow_destructive: true })), cmd).toBeNull();
    }
    expect(validateRule(bash('ls', { id: '' }))).not.toBeNull();
    expect(validateRule(bash('ls', { id: 'a b' }))).not.toBeNull();
    expect(validateRule(bash('ls', { tool: 'bash' }))).not.toBeNull();
    expect(validateRule(bash('ls', { tool: 'WebFetch' }))).not.toBeNull();
    for (const dir of ['../x', 'src/../../x', '/etc', 'a\0b']) {
      expect(validateRule({ id: 'r', project_id: 'p1', tool: 'Read', dir, prefix: false, allow_destructive: false }), dir).not.toBeNull();
    }
    expect(validateRule({ id: 'r', project_id: 'p1', tool: 'Read', command: 'ls', prefix: false, allow_destructive: false })).not.toBeNull();
  });
});

function bridge(initial: AutoAllowConfig = { enabled: false, rules: [] }) {
  const box = { config: initial };
  return {
    load: vi.fn<AutoAllowBridge['load']>(async () => box.config),
    save: vi.fn<AutoAllowBridge['save']>(async (c) => void (box.config = c)),
  };
}

const open = async () => {
  act(() => void emitUiCommand('openSettings'));
  await screen.findByRole('dialog', { name: 'Settings' });
  return screen.findByRole('region', { name: 'Auto-allow rules' });
};

describe('Settings › Agents › Auto-allow rules', () => {
  it('is off by default, loads the daemon config and warns what it does', async () => {
    const b = bridge();
    setAutoAllowBridge(b);
    mountWp5();
    const box = await open();
    const toggle = await within(box).findByRole('checkbox', { name: 'Auto-allow matching tool calls' });
    expect(toggle).not.toBeChecked();
    expect(within(box).getByText(/No rules/)).toBeInTheDocument();
    expect(within(box).getByText(/Off by default/)).toBeInTheDocument();
    expect(b.save).not.toHaveBeenCalled();
  });

  it('turning it on saves enabled with the rules as they are', async () => {
    const b = bridge({ enabled: false, rules: [bash('git status')] });
    setAutoAllowBridge(b);
    mountWp5();
    const box = await open();
    await userEvent.click(await within(box).findByRole('checkbox', { name: 'Auto-allow matching tool calls' }));
    await waitFor(() => expect(b.save).toHaveBeenCalledWith({ enabled: true, rules: [bash('git status')] }));
  });

  it('adds a Bash rule for one project and lists it', async () => {
    const b = bridge();
    setAutoAllowBridge(b);
    mountWp5();
    const box = await open();
    await within(box).findByRole('checkbox', { name: 'Auto-allow matching tool calls' });
    await userEvent.selectOptions(within(box).getByLabelText('Rule project'), 'billing-api');
    await userEvent.type(within(box).getByLabelText('Rule command'), 'git status');
    await userEvent.click(within(box).getByRole('button', { name: 'Add rule' }));
    await waitFor(() => expect(b.save).toHaveBeenCalledTimes(1));
    const saved = b.save.mock.calls[0][0];
    expect(saved.enabled).toBe(false);
    expect(saved.rules).toHaveLength(1);
    expect(saved.rules[0]).toMatchObject({ project_id: 'p1', tool: 'Bash', command: 'git status', prefix: false, allow_destructive: false });
    expect(within(box).getByText('git status')).toBeInTheDocument();
    expect(within(within(box).getByRole('list', { name: 'Rules' })).getByText('billing-api')).toBeInTheDocument();
  });

  it('adds a path rule with a project-relative directory', async () => {
    const b = bridge();
    setAutoAllowBridge(b);
    mountWp5();
    const box = await open();
    await within(box).findByRole('checkbox', { name: 'Auto-allow matching tool calls' });
    await userEvent.selectOptions(within(box).getByLabelText('Rule tool'), 'Read');
    expect(within(box).queryByLabelText('Rule command')).toBeNull();
    await userEvent.type(within(box).getByLabelText('Rule directory'), 'src');
    await userEvent.click(within(box).getByRole('button', { name: 'Add rule' }));
    await waitFor(() => expect(b.save).toHaveBeenCalledTimes(1));
    expect(b.save.mock.calls[0][0].rules[0]).toMatchObject({ project_id: 'p1', tool: 'Read', dir: 'src' });
  });

  it('refuses a chained command and a destructive one without the explicit box; nothing is saved', async () => {
    const b = bridge();
    setAutoAllowBridge(b);
    mountWp5();
    const box = await open();
    await within(box).findByRole('checkbox', { name: 'Auto-allow matching tool calls' });
    const cmd = within(box).getByLabelText('Rule command');
    await userEvent.type(cmd, 'git status; rm -rf ~');
    await userEvent.click(within(box).getByRole('button', { name: 'Add rule' }));
    expect(await within(box).findByRole('alert')).toHaveTextContent(/plain words/);
    await userEvent.clear(cmd);
    await userEvent.type(cmd, 'rm -rf build');
    await userEvent.click(within(box).getByRole('button', { name: 'Add rule' }));
    expect(await within(box).findByRole('alert')).toHaveTextContent(/destructive/);
    expect(b.save).not.toHaveBeenCalled();
    await userEvent.click(within(box).getByRole('checkbox', { name: 'Allow a destructive command or a secrets path' }));
    await userEvent.click(within(box).getByRole('button', { name: 'Add rule' }));
    await waitFor(() => expect(b.save).toHaveBeenCalledTimes(1));
  });

  it('removes a rule', async () => {
    const b = bridge({ enabled: true, rules: [bash('git status')] });
    setAutoAllowBridge(b);
    mountWp5();
    const box = await open();
    await userEvent.click(await within(box).findByRole('button', { name: 'Remove rule r1' }));
    await waitFor(() => expect(b.save).toHaveBeenCalledWith({ enabled: true, rules: [] }));
    expect(within(box).queryByText('git status')).toBeNull();
  });

  it('shows the daemon error and keeps the old rules when saving fails', async () => {
    const b = bridge({ enabled: false, rules: [bash('git status')] });
    b.save.mockRejectedValueOnce(new Error('saving the rules: disk full'));
    setAutoAllowBridge(b);
    mountWp5();
    const box = await open();
    await userEvent.click(await within(box).findByRole('checkbox', { name: 'Auto-allow matching tool calls' }));
    expect(await within(box).findByRole('alert')).toHaveTextContent('disk full');
    expect(within(box).getByRole('checkbox', { name: 'Auto-allow matching tool calls' })).not.toBeChecked();
    expect(within(box).getByText('git status')).toBeInTheDocument();
  });

  it('without a way to reach the daemon the controls are disabled and say so', async () => {
    setAutoAllowBridge(null);
    mountWp5();
    const box = await open();
    expect(within(box).getByRole('checkbox', { name: 'Auto-allow matching tool calls' })).toBeDisabled();
    expect(within(box).getByText(/not available yet/i)).toBeInTheDocument();
    expect(within(box).queryByRole('button', { name: 'Add rule' })).toBeNull();
  });
});
