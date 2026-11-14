import { act, screen, waitFor, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { afterEach, describe, expect, it, vi } from 'vitest';
import type { DaemonClient } from '../lib/client';
import { emitUiCommand } from '../lib/ui/commands';
import { mountWp5 } from '../sheets/harness';
import { FALLBACK_FONTS, loadInstalledFonts, setFontSource } from './fonts';
import type { InstalledFonts } from './fonts';
import { terminalStack } from './appearance';

const INSTALLED: InstalledFonts = {
  families: ['Arial', 'Fira Code', 'Helvetica Neue', 'Hack Nerd Font Mono', 'Zapfino'],
  monospace: ['Fira Code', 'Hack Nerd Font Mono'],
};

const openAppearance = async () => {
  act(() => void emitUiCommand('openSettings'));
  await screen.findByRole('dialog', { name: 'Settings' });
  await userEvent.click(screen.getByRole('button', { name: 'Appearance' }));
  return {
    iface: await screen.findByLabelText('Interface font'),
    term: screen.getByLabelText('Terminal font'),
  };
};
const optionTexts = (el: HTMLElement) => within(el).getAllByRole('option').map((o) => o.textContent);
const group = (el: HTMLElement, label: string) => {
  const g = el.querySelector<HTMLElement>(`optgroup[label="${label}"]`);
  if (!g) throw new Error(`no optgroup ${label}`);
  return g;
};

afterEach(() => setFontSource(null));

describe('font pickers', () => {
  it('list the installed fonts: System first, then a Monospace group, then Other fonts', async () => {
    setFontSource(async () => INSTALLED);
    mountWp5();
    const { iface, term } = await openAppearance();
    await waitFor(() => expect(optionTexts(iface)).toContain('Zapfino'));

    expect(optionTexts(iface)).toEqual(['System (SF Pro)', ...INSTALLED.families]);
    expect(optionTexts(group(term, 'Monospace'))).toEqual(['Fira Code', 'Hack Nerd Font Mono']);
    expect(optionTexts(group(term, 'Other fonts'))).toEqual(['Arial', 'Helvetica Neue', 'Zapfino']);
    // the groups come in this order, after any "(not installed)" entry
    expect([...term.querySelectorAll('optgroup')].map((g) => g.label)).toEqual(['Monospace', 'Other fonts']);
  });

  it('keep a stored font that is not installed selectable, marked "(not installed)"', async () => {
    setFontSource(async () => INSTALLED);
    mountWp5();
    const { term } = await openAppearance();
    // the MockClient's saved terminal font is SF Mono, which this Mac does not have
    await waitFor(() => expect(optionTexts(term)).toContain('Zapfino'));
    const missing = within(term).getByRole('option', { name: 'SF Mono (not installed)' });
    expect((missing as HTMLOptionElement).value).toBe('SF Mono');
    expect(term).toHaveValue('SF Mono');
    expect(optionTexts(term)[0]).toBe('SF Mono (not installed)');
  });

  describe('"(not installed)" is only said about a list that was really read', () => {
    const storeTerminalFont = async (client: DaemonClient, font: string) => {
      const s = await client.getSettings();
      s.appearance.terminal_font = font;
      await client.updateSettings(s);
    };

    it('while the lookup is still running, the stored font is a plain option, then it is judged against the real list', async () => {
      let answer: (f: InstalledFonts) => void = () => {};
      setFontSource(() => new Promise<InstalledFonts>((r) => (answer = r)));
      const { client } = mountWp5();
      await storeTerminalFont(client, 'Fira Code'); // not in the short fallback list, but installed
      const { term } = await openAppearance();
      await waitFor(() => expect(term).toHaveValue('Fira Code'));
      expect(optionTexts(term).join('|')).not.toContain('not installed');
      act(() => answer(INSTALLED));
      await waitFor(() => expect(optionTexts(term)).toContain('Zapfino'));
      expect(optionTexts(term).join('|')).not.toContain('not installed'); // Fira Code is installed
      expect(term).toHaveValue('Fira Code');
    });

    it('the same font flips to "(not installed)" only once the real list has answered and lacks it', async () => {
      let answer: (f: InstalledFonts) => void = () => {};
      setFontSource(() => new Promise<InstalledFonts>((r) => (answer = r)));
      mountWp5();
      const { term } = await openAppearance();
      expect(within(term).queryByRole('option', { name: 'SF Mono (not installed)' })).toBeNull();
      expect(term).toHaveValue('SF Mono');
      act(() => answer(INSTALLED));
      await screen.findByRole('option', { name: 'SF Mono (not installed)' });
    });

    it('a failed lookup says nothing about what is installed', async () => {
      setFontSource(() => Promise.reject(new Error('no host')));
      const { client } = mountWp5();
      await storeTerminalFont(client, 'Fira Code');
      const { term } = await openAppearance();
      await waitFor(() => expect(term).toHaveValue('Fira Code'));
      await new Promise((r) => setTimeout(r, 20)); // let the rejection settle
      expect(optionTexts(term).join('|')).not.toContain('not installed');
    });

    it('switching the theme meanwhile changes nothing about the font option', async () => {
      let answer: (f: InstalledFonts) => void = () => {};
      setFontSource(() => new Promise<InstalledFonts>((r) => (answer = r)));
      mountWp5();
      const { term } = await openAppearance();
      await userEvent.click(screen.getByRole('button', { name: 'Light' }));
      expect(optionTexts(term)).not.toContain('SF Mono (not installed)');
      expect(term).toHaveValue('SF Mono');
      act(() => answer({ ...INSTALLED, monospace: [...INSTALLED.monospace, 'SF Mono'], families: [...INSTALLED.families, 'SF Mono'] }));
      await waitFor(() => expect(optionTexts(term)).toContain('Zapfino'));
      expect(optionTexts(term)).not.toContain('SF Mono (not installed)');
    });
  });

  it('save a chosen installed family, and the preview uses it with SF Mono and Menlo behind it', async () => {
    setFontSource(async () => INSTALLED);
    const { client } = mountWp5();
    const { term } = await openAppearance();
    await waitFor(() => expect(optionTexts(term)).toContain('Fira Code'));
    await userEvent.selectOptions(term, 'Fira Code');
    await waitFor(async () => expect((await client.getSettings()).appearance.terminal_font).toBe('Fira Code'));
    await waitFor(() => expect(screen.getByTestId('font-preview').style.fontFamily).toMatch(/^"?Fira Code"?, "?SF Mono"?, Menlo/));
  });

  it('draw each option in its own font', async () => {
    setFontSource(async () => INSTALLED);
    mountWp5();
    const { iface } = await openAppearance();
    await waitFor(() => expect(optionTexts(iface)).toContain('Zapfino'));
    expect((within(iface).getByRole('option', { name: 'Zapfino' })).style.fontFamily).toContain('Zapfino');
  });

  it('use the short built-in lists in the browser build (no Tauri)', async () => {
    mountWp5();
    const { iface, term } = await openAppearance();
    expect(optionTexts(iface)).toEqual(['System (SF Pro)', ...FALLBACK_FONTS.families]);
    expect(optionTexts(group(term, 'Monospace'))).toEqual(FALLBACK_FONTS.monospace);
    expect(optionTexts(group(term, 'Other fonts'))).toEqual(['Helvetica Neue', 'Inter']);
    expect(term).toHaveValue('SF Mono');
  });

  it('fall back to the short lists when the lookup fails', async () => {
    setFontSource(() => Promise.reject(new Error('no host')));
    mountWp5();
    const { iface } = await openAppearance();
    await waitFor(() => expect(optionTexts(iface)).toEqual(['System (SF Pro)', ...FALLBACK_FONTS.families]));
  });

  it('ask once per Settings open, and again on the next open', async () => {
    const source = vi.fn(async () => INSTALLED);
    setFontSource(source);
    mountWp5();
    await openAppearance();
    await userEvent.click(screen.getByRole('button', { name: 'Notifications' }));
    await userEvent.click(screen.getByRole('button', { name: 'Appearance' }));
    await screen.findByLabelText('Terminal font');
    expect(source).toHaveBeenCalledTimes(1);
    await userEvent.keyboard('{Escape}');
    await openAppearance();
    await waitFor(() => expect(source).toHaveBeenCalledTimes(2));
  });
});

describe('terminalStack', () => {
  it('puts the chosen family first, then SF Mono, then Menlo', () => {
    expect(terminalStack('Fira Code')).toMatch(/^"Fira Code", "SF Mono", Menlo/);
  });
});

describe('loadInstalledFonts', () => {
  it('returns the fallback lists without Tauri', async () => {
    expect(await loadInstalledFonts()).toEqual(FALLBACK_FONTS);
  });
});
