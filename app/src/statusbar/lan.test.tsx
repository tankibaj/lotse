import { render, screen } from '@testing-library/react';
import { describe, expect, it, vi } from 'vitest';
import { RemoteSection } from '../settings/sections';
import { mount } from '../sidebar/testUtils';
import { StatusBar } from './StatusBar';

// The LAN word in the status bar and the Remote switch in Settings read one source (`settings/remote`).
const lan = vi.hoisted(() => ({ on: false }));
vi.mock('../settings/remote', () => ({
  lanState: () =>
    lan.on
      ? { on: true, label: 'LAN On', tooltip: 'LAN access is on (Settings › Remote)' }
      : { on: false, label: 'LAN Off', tooltip: 'LAN access is off (Settings › Remote)' },
}));

describe('LAN state', () => {
  it('is off in Alpha, with a tooltip that says where it lives', async () => {
    lan.on = false;
    mount(<StatusBar />);
    await screen.findByText(/lotsed running/);
    expect(screen.getByText('LAN Off')).toHaveAttribute('title', 'LAN access is off (Settings › Remote)');
  });

  it('follows the setting, in the status bar and in Settings › Remote alike', async () => {
    lan.on = true;
    const bar = mount(<StatusBar />);
    await screen.findByText(/lotsed running/);
    expect(screen.getByText('LAN On')).toHaveAttribute('title', 'LAN access is on (Settings › Remote)');
    expect(screen.queryByText('LAN Off')).toBeNull();
    bar.unmount();

    render(<RemoteSection />);
    expect(screen.getByRole('checkbox', { name: /Allow access from devices/ })).toBeChecked();
    lan.on = false;
  });
});
