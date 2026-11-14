import { act, screen, waitFor } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { describe, expect, it } from 'vitest';
import { emitUiCommand } from '../lib/ui/commands';
import { mountWp5 } from '../sheets/harness';

describe('Settings › Agents may start sessions without asking', () => {
  it('is off by default and saves when switched on and off again', async () => {
    const { client } = mountWp5();
    act(() => void emitUiCommand('openSettings'));
    await screen.findByRole('dialog', { name: 'Settings' });
    const box = await screen.findByRole('checkbox', { name: 'Agents may start sessions without asking' });
    expect(box).not.toBeChecked();
    await userEvent.click(box);
    await waitFor(async () => expect((await client.getSettings()).agents_start_sessions_without_asking).toBe(true));
    expect(box).toBeChecked();
    await userEvent.click(box);
    await waitFor(async () => expect((await client.getSettings()).agents_start_sessions_without_asking).toBe(false));
  });
});
