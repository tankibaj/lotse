import { act, fireEvent, screen, waitFor, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { describe, expect, it } from 'vitest';
import { emitUiCommand } from '../lib/ui/commands';
import { mountWp5 } from '../sheets/harness';
import { getOverlay } from '../sheets/overlay';
import { applyAppearance } from './appearance';

const open = async () => {
  act(() => void emitUiCommand('openSettings'));
  return screen.findByRole('dialog', { name: 'Settings' });
};
const nav = (name: string) => userEvent.click(screen.getByRole('button', { name }));

describe('Settings page', () => {
  it('opens on the command and ⌘, with Agents first; Esc and Back close it', async () => {
    mountWp5();
    await open();
    expect(screen.getByRole('heading', { name: 'Agents' })).toBeInTheDocument();
    expect(screen.getByRole('button', { name: 'Agents' })).toHaveAttribute('aria-current', 'page');
    await userEvent.keyboard('{Escape}');
    expect(getOverlay()).toBeNull();
    fireEvent.keyDown(window, { code: 'Comma', metaKey: true });
    expect(await screen.findByRole('dialog', { name: 'Settings' })).toBeInTheDocument();
    await userEvent.click(screen.getByRole('button', { name: /Back to sessions/ }));
    expect(getOverlay()).toBeNull();
  });

  it('takes focus from the terminal behind it, keeps Tab inside, and gives focus back', async () => {
    mountWp5();
    const term = document.createElement('textarea'); // stands in for xterm's input
    document.body.append(term);
    term.focus();
    const dlg = await open();
    expect(dlg.contains(document.activeElement)).toBe(true);
    for (let i = 0; i < 15; i++) {
      await userEvent.tab();
      expect(dlg.contains(document.activeElement)).toBe(true);
    }
    await userEvent.keyboard('{Escape}');
    expect(term).toHaveFocus();
    term.remove();
  });

  it('lists the six sections', async () => {
    mountWp5();
    await open();
    const names = within(screen.getByRole('navigation', { name: 'Settings sections' })).getAllByRole('button').map((b) => b.textContent);
    expect(names).toEqual(['Agents', 'Notifications', 'Appearance', 'Remote', 'Privacy', 'Shortcuts']);
  });

  describe('Agents', () => {
    it('shows each command with the detected version, or not found', async () => {
      mountWp5();
      await open();
      expect(await screen.findByLabelText('Terminal launch command')).toHaveValue('$SHELL -l');
      expect(screen.getByLabelText('Claude Code launch command')).toHaveValue('claude');
      expect(screen.getByText('zsh 5.9')).toBeInTheDocument();
      expect(screen.getByText('v2.1.309 · hooks installed')).toBeInTheDocument();
      expect(screen.getByText('v0.98 · OSC 7501')).toBeInTheDocument();
      expect(screen.getByText('not found on PATH')).toBeInTheDocument();
    });

    it('saves an edited command and Reset restores the default', async () => {
      const { client } = mountWp5();
      await open();
      const input = await screen.findByLabelText('Claude Code launch command');
      await userEvent.clear(input);
      await userEvent.type(input, 'claude --model opus');
      await userEvent.tab(); // blur flushes the debounce
      await waitFor(async () => expect((await client.getSettings()).launch_commands.claude_code).toBe('claude --model opus'));
      expect((await client.getSettings()).launch_commands.codex).toBe('codex');
      await userEvent.click(screen.getByRole('button', { name: 'Reset Claude Code command' }));
      expect(input).toHaveValue('claude');
      await waitFor(async () => expect((await client.getSettings()).launch_commands.claude_code).toBe('claude'));
    });

    it('an emptied command falls back to the default instead of saving nothing', async () => {
      const { client } = mountWp5();
      await open();
      const input = await screen.findByLabelText('Codex launch command');
      await userEvent.clear(input);
      await userEvent.tab();
      expect(input).toHaveValue('codex');
      expect((await client.getSettings()).launch_commands.codex).toBe('codex');
    });

    it('does not lose a pending edit when the page is closed', async () => {
      const { client } = mountWp5();
      await open();
      const input = await screen.findByLabelText('OpenCode launch command');
      await userEvent.type(input, ' --x');
      await userEvent.keyboard('{Escape}');
      await waitFor(async () => expect((await client.getSettings()).launch_commands.opencode).toBe('opencode --x'));
    });
  });

  describe('Notifications', () => {
    it('toggles are bound to the settings and saved', async () => {
      const { client } = mountWp5();
      await open();
      await nav('Notifications');
      const done = await screen.findByRole('checkbox', { name: 'Done sound' });
      expect(done).not.toBeChecked();
      expect(screen.getByRole('checkbox', { name: 'Needs permission sound' })).toBeChecked();
      await userEvent.click(done);
      await userEvent.click(screen.getByRole('checkbox', { name: 'Needs permission banner' }));
      await userEvent.click(screen.getByRole('checkbox', { name: 'Allow/Deny in banner' }));
      await userEvent.click(screen.getByRole('checkbox', { name: /Only when Lotse is not focused/ }));
      const n = (await client.getSettings()).notifications;
      expect(n.done.sound).toBe(true);
      expect(n.perm).toMatchObject({ banner: false, sound: true, allow_deny: false });
      expect(n.only_when_unfocused).toBe(false);
    });

    it('a sound pick saves sound_name; Default clears it; None turns the sound off', async () => {
      const { client } = mountWp5();
      await open();
      await nav('Notifications');
      const pick = await screen.findByRole('combobox', { name: 'Done sound name' });
      expect(pick).toHaveValue('none'); // done is silent by default
      expect(screen.getByRole('combobox', { name: 'Needs permission sound name' })).toHaveValue('default');
      const names = within(pick).getAllByRole('option').map((o) => o.textContent);
      expect(names.slice(0, 2)).toEqual(['None', 'Default']);
      expect(names).toContain('Glass');
      await userEvent.selectOptions(pick, 'Glass');
      expect((await client.getSettings()).notifications.done).toMatchObject({ sound: true, sound_name: 'Glass' });
      expect(screen.getByRole('checkbox', { name: 'Done sound' })).toBeChecked();
      await userEvent.selectOptions(pick, 'default');
      const done = (await client.getSettings()).notifications.done as unknown as Record<string, unknown>;
      expect(done.sound).toBe(true);
      expect(done.sound_name ?? null).toBeNull();
      await userEvent.selectOptions(pick, 'Glass');
      await userEvent.selectOptions(pick, 'none');
      expect((await client.getSettings()).notifications.done).toMatchObject({ sound: false });
      expect(screen.getByRole('checkbox', { name: 'Done sound' })).not.toBeChecked();
    });

    it('Allow/Deny exists for permission requests only', async () => {
      mountWp5();
      await open();
      await nav('Notifications');
      await screen.findByRole('checkbox', { name: 'Allow/Deny in banner' });
      expect(screen.getAllByRole('checkbox', { name: 'Allow/Deny in banner' })).toHaveLength(1);
      expect(screen.getAllByText('—')).toHaveLength(3);
    });
  });

  describe('Appearance', () => {
    it('Theme switches the class on <html> and is saved', async () => {
      const { client } = mountWp5();
      await open();
      await nav('Appearance');
      await userEvent.click(screen.getByRole('button', { name: 'Dark' }));
      expect(document.documentElement).toHaveClass('dark');
      expect(screen.getByRole('button', { name: 'Dark' })).toHaveAttribute('aria-pressed', 'true');
      await userEvent.click(screen.getByRole('button', { name: 'Light' }));
      expect(document.documentElement).not.toHaveClass('dark');
      expect((await client.getSettings()).appearance.theme).toBe('light');
    });

    it('both fonts and sizes are saved, applied, and shown in the preview', async () => {
      const { client } = mountWp5();
      await open();
      await nav('Appearance');
      await userEvent.selectOptions(await screen.findByLabelText('Terminal font'), 'Menlo');
      await userEvent.selectOptions(screen.getByLabelText('Interface font'), 'Inter');
      fireEvent.change(screen.getByLabelText('Terminal font size'), { target: { value: '16' } });
      fireEvent.change(screen.getByLabelText('Interface font size'), { target: { value: '15' } });
      await userEvent.keyboard('{Escape}'); // flushes the slider debounce
      await waitFor(async () => {
        const a = (await client.getSettings()).appearance;
        expect(a).toMatchObject({ terminal_font: 'Menlo', interface_font: 'Inter', terminal_font_size: 16, interface_font_size: 15 });
      });
      await waitFor(() => expect(document.body.style.fontSize).toBe('15px'));
      expect(document.body.style.fontFamily).toContain('Inter');
      expect(document.documentElement.style.getPropertyValue('--lotse-terminal-font')).toContain('Menlo');
      expect(document.documentElement.style.getPropertyValue('--lotse-terminal-font-size')).toBe('16px');
    });

    it('applyAppearance clamps sizes and drops the override for the system font', () => {
      applyAppearance({ theme: 'auto', interface_font: 'System (SF Pro)', interface_font_size: 99, terminal_font: 'SF Mono', terminal_font_size: 1 });
      expect(document.body.style.fontSize).toBe('17px');
      expect(document.body.style.fontFamily).toBe('');
      expect(document.documentElement.style.getPropertyValue('--lotse-terminal-font-size')).toBe('10px');
      document.body.removeAttribute('style');
    });
  });

  it('Remote is shown Off and cannot be switched on', async () => {
    mountWp5();
    await open();
    await nav('Remote');
    const box = screen.getByRole('checkbox', { name: /Allow access from devices/ });
    expect(box).not.toBeChecked();
    expect(box).toBeDisabled();
    expect(screen.getByText(/opens no network listener/)).toBeInTheDocument();
    expect(screen.queryByText(/Alpha/)).toBeNull(); // this is the Beta
    expect(screen.getByText(/Not available yet/)).toBeInTheDocument();
  });

  it('Privacy states zero telemetry', async () => {
    mountWp5();
    await open();
    await nav('Privacy');
    expect(screen.getByText('Zero telemetry.')).toBeInTheDocument();
    expect(screen.getByText(/Nothing leaves this Mac/)).toBeInTheDocument();
  });

  it('Shortcuts lists the main bindings', async () => {
    mountWp5();
    await open();
    await nav('Shortcuts');
    for (const k of ['⌘K', '⌘J', '⌘T', '⇧⌘O', '⌘,', '⌃⌘S', '⌘/']) expect(screen.getByText(k)).toBeInTheDocument();
  });

  it('Shortcuts has the real panel keys (Changes is ⇧⌘G), the composer and the review (⇧⌘R)', async () => {
    mountWp5();
    await open();
    await nav('Shortcuts');
    expect(screen.getByText('⇧⌘G · ⇧⌘F · ⇧⌘T · ⇧⌘U · ⇧⌘I')).toBeInTheDocument();
    expect(screen.getByText('Open prompt composer').nextElementSibling).toHaveTextContent('⌘⇧↩');
    expect(screen.getByText('Full diff review').nextElementSibling).toHaveTextContent('⇧⌘R');
    expect(screen.queryByText(/⇧⌘D/)).toBeNull();
    expect(screen.queryByText(/⌥⌘D/)).toBeNull();
  });

  it('the nav marks only the section on screen as current, after any number of clicks', async () => {
    mountWp5();
    await open();
    const current = () =>
      within(screen.getByRole('navigation', { name: 'Settings sections' }))
        .getAllByRole('button')
        .filter((b) => b.getAttribute('aria-current') === 'page')
        .map((b) => b.textContent);
    expect(current()).toEqual(['Agents']);
    await nav('Notifications');
    expect(current()).toEqual(['Notifications']);
    await nav('Appearance');
    expect(current()).toEqual(['Appearance']);
    expect(screen.getByRole('heading', { name: 'Appearance' })).toBeInTheDocument();
    // a section is not a toggle: nothing in the nav reports a pressed state that could go stale
    expect(within(screen.getByRole('navigation', { name: 'Settings sections' })).queryAllByRole('button', { pressed: true })).toHaveLength(0);
  });
});
