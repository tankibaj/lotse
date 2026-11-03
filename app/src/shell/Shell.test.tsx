import { readFileSync } from 'node:fs';
import { act, fireEvent, render, screen, waitFor } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { ClientProvider, MockClient, useClient } from '../lib/client';
import { emitUiCommand, onUiCommand } from '../lib/ui/commands';
import { closePanel, closeReview } from '../lib/ui/panel';
import { selectSession } from '../lib/ui/selection';
import { Shell } from './Shell';

// The drop watcher of the sheets talks to the webview when it believes it runs inside Tauri (one test fakes that).
vi.mock('@tauri-apps/api/webview', () => ({ getCurrentWebview: () => ({ onDragDropEvent: async () => () => {} }) }));

const mount = () => {
  const client = new MockClient({ script: false });
  return render(
    <ClientProvider client={client}>
      <Shell />
    </ClientProvider>,
  );
};

afterEach(() => localStorage.clear());

describe('Shell', () => {
  it('mounts every slot with its placeholder', () => {
    const { container } = mount();
    for (const slot of ['sidebar', 'tabs', 'panes', 'statusbar', 'sheets', 'settings', 'palette']) {
      expect(container.querySelector(`[data-slot="${slot}"]`), slot).not.toBeNull();
    }
    expect(screen.getByTestId('terminal-placeholder')).toBeInTheDocument();
    expect(screen.getByRole('button', { name: 'New session' })).toBeInTheDocument();
    expect(screen.getByRole('contentinfo')).toHaveTextContent('LAN Off');
  });

  it('both title bars drag the window from any non-button spot (title text, empty tab strip)', () => {
    const { container } = mount();
    const bars = container.querySelectorAll('[data-tauri-drag-region]');
    expect(bars).toHaveLength(2);
    // a bare attribute only drags on the bar itself; "deep" covers its children, buttons still click
    bars.forEach((bar) => expect(bar).toHaveAttribute('data-tauri-drag-region', 'deep'));
    expect(bars[1]?.querySelector('[data-slot="tabs"]')).not.toBeNull();
  });

  it('the native traffic lights are positioned in tauri.conf.json (centre line 22 px)', () => {
    const conf = JSON.parse(readFileSync('src-tauri/tauri.conf.json', 'utf8') /* vitest runs from app/ */) as {
      app: { windows: { titleBarStyle: string; trafficLightPosition?: { x: number; y: number } }[] };
    };
    const win = conf.app.windows[0];
    expect(win?.titleBarStyle).toBe('Overlay');
    expect(win?.trafficLightPosition).toEqual({ x: 12, y: 24 });
  });

  it('with the sidebar hidden the title bar keeps one centre line: lights spacer, the sidebar icon button, then the tabs', async () => {
    (window as unknown as Record<string, unknown>).__TAURI_INTERNALS__ = {};
    try {
      const { container } = mount();
      const hideIcon = screen.getByRole('button', { name: /Hide sidebar/ }).querySelector('svg')?.outerHTML;
      expect(hideIcon).toBeTruthy();
      await userEvent.click(screen.getByRole('button', { name: /Hide sidebar/ }));
      const show = screen.getByRole('button', { name: 'Show sidebar' });
      expect(show).not.toHaveTextContent('☰');
      expect(show.querySelector('svg')?.outerHTML).toBe(hideIcon); // the same glyph as the hide button
      const row = container.querySelectorAll('[data-tauri-drag-region]')[0] as HTMLElement;
      expect(row).toHaveAttribute('data-tauri-drag-region', 'deep');
      expect(row).toHaveClass('items-center', 'pt-px'); // centred on y = 22 (the 1 px border is below)
      // lights spacer (52 px), then the toggle, then the tabs slot, like the sidebar header's title
      expect([...row.children].map((c) => c.getAttribute('aria-hidden') ?? c.getAttribute('aria-label') ?? c.getAttribute('data-slot'))).toEqual(['true', 'Show sidebar', 'tabs', 'Panels']);
    } finally {
      delete (window as unknown as Record<string, unknown>).__TAURI_INTERNALS__;
    }
  });

  it('the tab strip, "+" and panel toolbar sit on the same centre line as the sidebar header (no bottom-aligned margins)', async () => {
    const { container } = mount();
    const tablist = await screen.findByRole('tablist');
    const [side, main] = container.querySelectorAll('[data-tauri-drag-region]');
    expect(side).toHaveClass('items-center');
    expect(main).toHaveClass('items-center', 'pt-px');
    expect(screen.getByRole('toolbar', { name: 'Panels' })).not.toHaveClass('mb-2');
    expect(screen.getByRole('button', { name: 'New session' })).not.toHaveClass('mb-2');
    expect(tablist).toHaveClass('self-stretch'); // tabs stay bottom-attached, full bar height
  });

  it('shows the mock\'s five panel buttons, enabled, with their shortcut in the tooltip (Changes is ⇧⌘G)', () => {
    mount();
    const bar = screen.getByRole('toolbar', { name: 'Panels' });
    const buttons = [...bar.querySelectorAll('button')];
    expect(buttons.map((b) => b.getAttribute('aria-label'))).toEqual(['Session', 'Changes', 'Files', 'Transcript', 'Usage and time']);
    expect(buttons.map((b) => b.title)).toEqual(['Session ⇧⌘I', 'Changes ⇧⌘G', 'Files ⇧⌘F', 'Transcript ⇧⌘T', 'Usage & time ⇧⌘U']);
    for (const b of buttons) {
      expect(b).not.toHaveAttribute('aria-disabled');
      expect(b).toHaveAttribute('aria-pressed', 'false');
    }
  });

  it('the settings and search buttons raise UI commands without knowing who listens', async () => {
    const settings = vi.fn();
    const palette = vi.fn();
    const off = [onUiCommand('openSettings', settings), onUiCommand('openPalette', palette)];
    mount();
    await userEvent.click(screen.getByRole('button', { name: 'Settings' }));
    await userEvent.click(screen.getByRole('button', { name: /Search/ }));
    expect(settings).toHaveBeenCalledTimes(1);
    expect(palette).toHaveBeenCalledTimes(1);
    off.forEach((f) => f());
  });

  it('hides and shows the sidebar', async () => {
    mount();
    expect(screen.getByLabelText('Sidebar')).toBeInTheDocument();
    await userEvent.click(screen.getByRole('button', { name: /Hide sidebar/ }));
    expect(screen.queryByLabelText('Sidebar')).toBeNull();
    await userEvent.click(screen.getByRole('button', { name: 'Show sidebar' }));
    expect(screen.getByLabelText('Sidebar')).toBeInTheDocument();
  });

  it('restores a saved sidebar width and rejects out-of-range values', () => {
    localStorage.setItem('lotse-side', '320');
    const first = mount();
    expect(screen.getByLabelText('Sidebar')).toHaveStyle({ width: '320px' });
    first.unmount();
    localStorage.setItem('lotse-side', '900');
    mount();
    expect(screen.getByLabelText('Sidebar')).toHaveStyle({ width: '260px' });
  });
});

describe('Shell panels', () => {
  const panel = () => document.querySelector('[data-slot="panel"] aside');
  const press = (code: string, mods: Partial<KeyboardEventInit> = { metaKey: true, shiftKey: true }) => fireEvent.keyDown(window, { code, ...mods });

  // the selection, panel and review live in module stores: start each test clean
  beforeEach(() => selectSession(null));
  afterEach(() => {
    act(() => {
      closePanel();
      closeReview();
      selectSession(null);
    });
  });

  it('the toolbar button opens the panel (340 px) and the same button closes it', async () => {
    mount();
    expect(panel()).toBeNull();
    const files = screen.getByRole('button', { name: 'Files' });
    await userEvent.click(files);
    expect(panel()).toHaveAttribute('aria-label', 'Files');
    expect(panel()).toHaveClass('w-[340px]');
    expect(files).toHaveAttribute('aria-pressed', 'true');
    await userEvent.click(files);
    expect(panel()).toBeNull();
    expect(files).toHaveAttribute('aria-pressed', 'false');
  });

  it('shows one panel at a time: another button swaps it, never stacks', async () => {
    mount();
    await userEvent.click(screen.getByRole('button', { name: 'Files' }));
    await userEvent.click(screen.getByRole('button', { name: 'Transcript' }));
    expect(document.querySelectorAll('[data-slot="panel"] aside')).toHaveLength(1);
    expect(panel()).toHaveAttribute('aria-label', 'Transcript');
    expect(screen.getByRole('button', { name: 'Files' })).toHaveAttribute('aria-pressed', 'false');
  });

  it.each([
    ['KeyI', 'Session'],
    ['KeyG', 'Changes'],
    ['KeyF', 'Files'],
    ['KeyT', 'Transcript'],
    ['KeyU', 'Usage and time'],
  ])('⇧⌘ %s opens the %s panel and toggles it closed', (code, name) => {
    mount();
    press(code);
    expect(panel()).toHaveAttribute('aria-label', name === 'Usage and time' ? 'Usage & time' : name);
    press(code);
    expect(panel()).toBeNull();
  });

  it('⇧⌘D no longer opens Changes (⌘⇧D is split down)', () => {
    mount();
    press('KeyD');
    expect(panel()).toBeNull();
  });

  it('the togglePanel command opens a panel by name and ignores unknown names', () => {
    mount();
    act(() => void emitUiCommand('togglePanel', 'usage'));
    expect(panel()).toHaveAttribute('aria-label', 'Usage & time');
    act(() => void emitUiCommand('togglePanel', 'nope'));
    expect(panel()).toHaveAttribute('aria-label', 'Usage & time');
  });

  it('openReview shows the full-window review with the session title (not its id); Esc closes it', async () => {
    mount();
    const review = () => document.querySelector('[data-slot="review"] [role="dialog"]');
    expect(review()).toBeNull();
    act(() => void emitUiCommand('openReview', 'a1', 'src/a.ts'));
    expect(review()).toHaveAttribute('aria-label', 'Review');
    await waitFor(() => expect(review()).toHaveTextContent('Fix invoice rounding · src/a.ts'));
    expect(review()).not.toHaveTextContent('a1');
    fireEvent.keyDown(window, { key: 'Escape' });
    expect(review()).toBeNull();
  });

  it('the review takes focus when it opens and gives it back to the terminal when it closes', async () => {
    mount();
    const term = document.createElement('textarea'); // stands in for xterm's input
    document.body.append(term);
    term.focus();
    act(() => void emitUiCommand('openReview', 'a1'));
    const dialog = document.querySelector('[data-slot="review"] [role="dialog"]') as HTMLElement;
    expect(dialog.contains(document.activeElement)).toBe(true);
    fireEvent.keyDown(term, { key: 'Escape' });
    expect(term).toHaveFocus();
    term.remove();
  });

  it('Back closes the review too', async () => {
    mount();
    act(() => void emitUiCommand('openReview', 'a1'));
    await userEvent.click(screen.getByRole('button', { name: /Back/ }));
    expect(document.querySelector('[data-slot="review"] [role="dialog"]')).toBeNull();
  });

  it('Esc closes the review even when the terminal swallows it (xterm stops the bubble)', () => {
    mount();
    act(() => void emitUiCommand('openReview', 'a1'));
    const term = document.createElement('textarea');
    term.addEventListener('keydown', (e) => e.stopPropagation());
    document.body.append(term);
    fireEvent.keyDown(term, { key: 'Escape' });
    expect(document.querySelector('[data-slot="review"] [role="dialog"]')).toBeNull();
    term.remove();
  });

  it('⇧⌘R reviews the selected session, and does nothing without one', async () => {
    mount();
    press('KeyR');
    expect(document.querySelector('[data-slot="review"] [role="dialog"]')).toBeNull();
    act(() => selectSession('a2'));
    press('KeyR');
    await waitFor(() => expect(document.querySelector('[data-slot="review"] [role="dialog"]')).toHaveTextContent('Flyway migration V42'));
    act(() => selectSession(null));
  });

  it('⌥⌘D (macOS Show/Hide Dock) no longer opens the review', () => {
    mount();
    act(() => selectSession('a2'));
    press('KeyD', { metaKey: true, altKey: true });
    expect(document.querySelector('[data-slot="review"] [role="dialog"]')).toBeNull();
    act(() => selectSession(null));
  });

  it('⌘⇧Return raises openComposer', () => {
    const open = vi.fn();
    const off = onUiCommand('openComposer', open);
    mount();
    press('Enter');
    expect(open).toHaveBeenCalledTimes(1);
    off();
  });
});

describe('ClientProvider', () => {
  it('gives UI code the DaemonClient and fails loudly without one', async () => {
    function Probe() {
      const c = useClient();
      return <span>{typeof c.getSnapshot}</span>;
    }
    render(
      <ClientProvider client={new MockClient({ script: false })}>
        <Probe />
      </ClientProvider>,
    );
    expect(screen.getByText('function')).toBeInTheDocument();
    const err = vi.spyOn(console, 'error').mockImplementation(() => {});
    expect(() => render(<Probe />)).toThrow(/no ClientProvider/);
    err.mockRestore();
  });
});
