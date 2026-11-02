import { render, screen, waitFor } from '@testing-library/react';
import { StrictMode } from 'react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { ClientProvider, MockClient } from '../lib/client';
import { FakeTerminal } from './fakeXterm';
import { TerminalView } from './TerminalView';

vi.mock('@xterm/xterm', async () => (await import('./fakeXterm')).xtermModule);
vi.mock('@xterm/addon-fit', async () => (await import('./fakeXterm')).fitModule);
vi.mock('@xterm/addon-web-links', async () => (await import('./fakeXterm')).linksModule);
vi.mock('@xterm/addon-webgl', async () => (await import('./fakeXterm')).webglModule);

let client: MockClient;
const mount = (ui = <TerminalView sessionId="a1" />, strict = false) => {
  const tree = <ClientProvider client={client}>{ui}</ClientProvider>;
  return render(strict ? <StrictMode>{tree}</StrictMode> : tree);
};

beforeEach(() => {
  FakeTerminal.reset();
  client = new MockClient({ script: false });
});
afterEach(() => client.close());

describe('TerminalView', () => {
  it('attaches the session and shows the replay', async () => {
    const attach = vi.spyOn(client, 'attach');
    mount();
    await waitFor(() => expect(FakeTerminal.last.text).toContain('Claude Code'));
    expect(attach).toHaveBeenCalledWith('a1', 80, 24);
    expect(screen.getByRole('group', { name: 'Terminal' })).toBeInTheDocument();
  });

  it('typing round-trips through the daemon: keystrokes go to the PTY and the echo is written back', async () => {
    mount();
    await waitFor(() => expect(FakeTerminal.last.text).toContain('Claude Code'));
    FakeTerminal.last.type('ls');
    await waitFor(() => expect(FakeTerminal.last.text.endsWith('ls')).toBe(true));
  });

  it('sends Enter as \\r and binary input byte for byte', async () => {
    const write = vi.spyOn(client, 'writePty');
    mount();
    await waitFor(() => expect(FakeTerminal.last.text).toContain('Claude Code'));
    FakeTerminal.last.type('\r');
    FakeTerminal.last.sendBinary('\x1b[M\xff');
    expect(Array.from(write.mock.calls[0]?.[1] ?? [])).toEqual([13]);
    expect(Array.from(write.mock.calls[1]?.[1] ?? [])).toEqual([0x1b, 0x5b, 0x4d, 0xff]);
  });

  it('tells the daemon about resizes after attach', async () => {
    const resize = vi.spyOn(client, 'resize');
    mount();
    await waitFor(() => expect(FakeTerminal.last.text).toContain('Claude Code'));
    FakeTerminal.last.resizeTo(100, 30);
    expect(resize).toHaveBeenCalledWith('a1', 100, 30);
  });

  it('detaches and disposes on unmount', async () => {
    const detach = vi.spyOn(client, 'detach');
    const { unmount } = mount();
    await waitFor(() => expect(FakeTerminal.last.text).toContain('Claude Code'));
    unmount();
    expect(detach).toHaveBeenCalledWith('a1');
    expect(FakeTerminal.last.disposed).toBe(true);
  });

  it('replays once under StrictMode (the discarded first mount never attaches)', async () => {
    const attach = vi.spyOn(client, 'attach');
    mount(<TerminalView sessionId="a1" />, true);
    await waitFor(() => expect(FakeTerminal.last.text).toContain('Claude Code'));
    expect(attach).toHaveBeenCalledTimes(1);
    expect(FakeTerminal.last.text.match(/Claude Code/g)).toHaveLength(1);
  });

  it('starts from the replay again after the link comes back', async () => {
    const attach = vi.spyOn(client, 'attach');
    mount();
    await waitFor(() => expect(FakeTerminal.last.text).toContain('Claude Code'));
    client.setConnection('reconnected');
    expect(FakeTerminal.last.resets).toBe(1);
    await waitFor(() => expect(attach).toHaveBeenCalledTimes(2));
    await waitFor(() => expect(FakeTerminal.last.text).toContain('Claude Code'));
  });

  it('input and resizes reach the daemon after a failed first attach was recovered by a reconnect', async () => {
    vi.spyOn(client, 'attach').mockRejectedValueOnce(new Error('busy'));
    const write = vi.spyOn(client, 'writePty');
    const resize = vi.spyOn(client, 'resize');
    mount();
    await waitFor(() => expect(FakeTerminal.last.text).toContain('attach failed: busy'));
    FakeTerminal.last.type('lost');
    expect(write).not.toHaveBeenCalled(); // not attached yet: nothing to send to
    client.setConnection('reconnected');
    await waitFor(() => expect(FakeTerminal.last.text).toContain('Claude Code'));
    FakeTerminal.last.type('ls');
    FakeTerminal.last.resizeTo(90, 20);
    expect(write).toHaveBeenCalledTimes(1);
    expect(resize).toHaveBeenCalledWith('a1', 90, 20);
  });

  it('detaches even when the pane goes away while the attach is still in flight', async () => {
    const attach = vi.spyOn(client, 'attach').mockReturnValueOnce(new Promise(() => {}));
    const detach = vi.spyOn(client, 'detach');
    const { unmount } = mount();
    await waitFor(() => expect(attach).toHaveBeenCalledTimes(1));
    unmount();
    expect(detach).toHaveBeenCalledWith('a1');
  });

  it('writes the error into the terminal when attach fails', async () => {
    mount(<TerminalView sessionId="nope" />);
    await waitFor(() => expect(FakeTerminal.last.text).toContain('attach failed: no session nope'));
  });

  it('applies font settings to the live terminal', () => {
    const { rerender } = mount(<TerminalView sessionId="a1" fontSize={13} />);
    expect(FakeTerminal.last.options.fontSize).toBe(13);
    rerender(
      <ClientProvider client={client}>
        <TerminalView sessionId="a1" fontSize={16} fontFamily='"Menlo", monospace' />
      </ClientProvider>,
    );
    expect(FakeTerminal.last.options.fontSize).toBe(16);
    expect(FakeTerminal.last.options.fontFamily).toBe('"Menlo", monospace');
  });

  it('focuses the terminal only when it is the focused pane', () => {
    const { rerender } = mount(<TerminalView sessionId="a1" focused={false} />);
    expect(FakeTerminal.last.focused).toBe(0);
    rerender(
      <ClientProvider client={client}>
        <TerminalView sessionId="a1" focused />
      </ClientProvider>,
    );
    expect(FakeTerminal.last.focused).toBe(1);
  });

  it('lets app shortcuts (⌘D, ⌘W, ⌘[) through to the window but keeps ⌘C / ⌘V and plain keys', () => {
    mount();
    const handle = FakeTerminal.last.keyHandler;
    const key = (init: KeyboardEventInit) => handle?.(new KeyboardEvent('keydown', init));
    expect(key({ key: 'd', metaKey: true })).toBe(false);
    expect(key({ key: 'w', metaKey: true })).toBe(false);
    expect(key({ key: '[', metaKey: true })).toBe(false);
    expect(key({ key: 'c', metaKey: true })).toBe(true);
    expect(key({ key: 'v', metaKey: true })).toBe(true);
    expect(key({ key: 'd', ctrlKey: true })).toBe(true);
    expect(key({ key: 'a' })).toBe(true);
  });

  it('routes OSC 8 hyperlinks through openExternal: ⌘-click on http(s) only', () => {
    const open = vi.spyOn(window, 'open').mockReturnValue(null);
    mount();
    const handler = FakeTerminal.last.options.linkHandler as {
      activate: (e: Pick<MouseEvent, 'metaKey' | 'ctrlKey'>, uri: string) => void;
    };
    expect(handler).toBeDefined();
    handler.activate({ metaKey: false, ctrlKey: false }, 'https://example.com/plain');
    handler.activate({ metaKey: true, ctrlKey: false }, 'file:///etc/passwd');
    handler.activate({ metaKey: true, ctrlKey: false }, 'javascript:alert(1)');
    expect(open).not.toHaveBeenCalled();
    handler.activate({ metaKey: true, ctrlKey: false }, 'https://example.com/a');
    expect(open).toHaveBeenCalledWith('https://example.com/a', '_blank', 'noopener,noreferrer');
    open.mockRestore();
  });

  it('says so when the terminal cannot start', async () => {
    FakeTerminal.failOpen = 'no canvas';
    mount();
    expect(await screen.findByRole('alert')).toHaveTextContent('Terminal unavailable: no canvas');
  });
});
