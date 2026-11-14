import { act, screen, waitFor, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { afterEach, describe, expect, it, vi } from 'vitest';
import { emitUiCommand } from '../lib/ui/commands';
import { mountWp5 } from '../sheets/harness';
import { setAutoAllowBridge } from './autoallow';
import type { AutoAllowBridge, AutoAllowConfig } from './autoallow';

// More panel cases on top of `autoallow.test.tsx`: what must never reach the daemon, and the preview.

afterEach(() => setAutoAllowBridge(undefined));

function bridge() {
  const box: { config: AutoAllowConfig } = { config: { enabled: false, rules: [] } };
  return {
    load: vi.fn<AutoAllowBridge['load']>(async () => box.config),
    save: vi.fn<AutoAllowBridge['save']>(async (c) => void (box.config = c)),
  };
}

const open = async () => {
  act(() => void emitUiCommand('openSettings'));
  await screen.findByRole('dialog', { name: 'Settings' });
  const box = await screen.findByRole('region', { name: 'Auto-allow rules' });
  await within(box).findByRole('checkbox', { name: 'Auto-allow matching tool calls' });
  return box;
};

describe('Auto-allow panel, adversarial input', () => {
  it('never saves a command with shell syntax, however it is spelled', async () => {
    const b = bridge();
    setAutoAllowBridge(b);
    mountWp5();
    const box = await open();
    const cmd = within(box).getByLabelText('Rule command');
    for (const bad of ['ls; id', 'ls && id', 'ls | sh', 'echo $(id)', 'echo `id`', 'ls > x', 'ls ../x', 'ls -l']) {
      await userEvent.clear(cmd);
      await userEvent.type(cmd, bad);
      await userEvent.click(within(box).getByRole('button', { name: 'Add rule' }));
      expect(await within(box).findByRole('alert'), bad).toBeInTheDocument();
    }
    expect(b.save).not.toHaveBeenCalled();
    expect(within(box).getByText(/No rules/)).toBeInTheDocument();
  });

  it('never saves a folder that leaves the project', async () => {
    const b = bridge();
    setAutoAllowBridge(b);
    mountWp5();
    const box = await open();
    await userEvent.selectOptions(within(box).getByLabelText('Rule tool'), 'Edit');
    const dir = within(box).getByLabelText('Rule directory');
    for (const bad of ['../x', '/etc', 'src/../../x']) {
      await userEvent.clear(dir);
      await userEvent.type(dir, bad);
      await userEvent.click(within(box).getByRole('button', { name: 'Add rule' }));
      expect(await within(box).findByRole('alert'), bad).toHaveTextContent(/relative to the project/);
    }
    expect(b.save).not.toHaveBeenCalled();
  });

  it('saves a prefix rule as a prefix rule, and the next rule gets its own id', async () => {
    const b = bridge();
    setAutoAllowBridge(b);
    mountWp5();
    const box = await open();
    const cmd = within(box).getByLabelText('Rule command');
    await userEvent.type(cmd, 'git status');
    await userEvent.click(within(box).getByRole('checkbox', { name: 'Also with plain arguments' }));
    await userEvent.click(within(box).getByRole('button', { name: 'Add rule' }));
    await waitFor(() => expect(b.save).toHaveBeenCalledTimes(1));
    expect(b.save.mock.calls[0]![0].rules[0]).toMatchObject({ id: 'r1', command: 'git status', prefix: true });
    await userEvent.type(cmd, 'git diff');
    await userEvent.click(within(box).getByRole('button', { name: 'Add rule' }));
    await waitFor(() => expect(b.save).toHaveBeenCalledTimes(2));
    expect(b.save.mock.calls[1]![0].rules.map((r) => r.id)).toEqual(['r1', 'r2']);
  });

  it('in the browser build says that nothing is saved', async () => {
    mountWp5();
    const box = await open();
    expect(within(box).getByText(/Preview: nothing is saved/)).toBeInTheDocument();
    expect(within(box).getByRole('checkbox', { name: 'Auto-allow matching tool calls' })).not.toBeChecked();
  });
});
