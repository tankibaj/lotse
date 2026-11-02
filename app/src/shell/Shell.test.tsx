import { render, screen } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { afterEach, describe, expect, it, vi } from 'vitest';
import { ClientProvider, MockClient, useClient } from '../lib/client';
import { onUiCommand } from '../lib/ui/commands';
import { Shell } from './Shell';

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
