import { readFileSync } from 'node:fs';
import { render, screen } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { afterEach, describe, expect, it, vi } from 'vitest';
import { ClientProvider, MockClient, useClient } from '../lib/client';
import { onUiCommand } from '../lib/ui/commands';
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

  it('shows the mock\'s five panel buttons, inert with a "Coming soon" tooltip until the Beta panels land', async () => {
    mount();
    const bar = screen.getByRole('toolbar', { name: 'Panels' });
    const buttons = [...bar.querySelectorAll('button')];
    expect(buttons.map((b) => b.getAttribute('aria-label'))).toEqual(['Session', 'Changes', 'Files', 'Transcript', 'Usage and time']);
    for (const b of buttons) {
      expect(b).toHaveAttribute('aria-disabled', 'true');
      expect(b.title).toMatch(/Coming soon$/);
    }
    const commands = vi.fn();
    const off = (['openSettings', 'openPalette', 'openNewSession', 'openAddProject', 'toggleSidebar'] as const).map((c) => onUiCommand(c, commands));
    await userEvent.click(buttons[0]!);
    expect(commands).not.toHaveBeenCalled();
    off.forEach((f) => f());
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
