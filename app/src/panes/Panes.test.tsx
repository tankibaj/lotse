import { act, fireEvent, render, screen, waitFor, within } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { ClientProvider, MockClient } from '../lib/client';
import { emitUiCommand } from '../lib/ui/commands';
import { getSelectedSessionId, selectSession } from '../lib/ui/selection';
import { FakeTerminal } from '../terminal/fakeXterm';
import { Panes, SESSION_DRAG_TYPE } from './Panes';
import { openInSplit } from './splitBus';
import { TabStrip } from './TabStrip';

vi.mock('@xterm/xterm', async () => (await import('../terminal/fakeXterm')).xtermModule);
vi.mock('@xterm/addon-fit', async () => (await import('../terminal/fakeXterm')).fitModule);
vi.mock('@xterm/addon-web-links', async () => (await import('../terminal/fakeXterm')).linksModule);
vi.mock('@xterm/addon-webgl', async () => (await import('../terminal/fakeXterm')).webglModule);

let client: MockClient;

const mount = async (extra?: React.ReactNode) => {
  const view = render(
    <ClientProvider client={client}>
      <Panes />
      {extra}
    </ClientProvider>,
  );
  await waitFor(() => expect(document.querySelector('[data-pane]')).not.toBeNull());
  return view;
};
const paneIds = () => [...document.querySelectorAll('[data-pane]')].map((p) => p.getAttribute('data-pane'));
/** US layout: the character each physical key types. Shortcuts follow the character, so other layouts pass `key` explicitly. */
const US_CHARS: Record<string, string> = { KeyD: 'd', KeyW: 'w', BracketLeft: '[', BracketRight: ']', Minus: '-', Digit0: '0', Equal: '=', NumpadAdd: '+' };
const key = (code: string, init: KeyboardEventInit = {}) =>
  fireEvent.keyDown(window, { code, key: US_CHARS[code] ?? code, metaKey: true, ...init });

beforeEach(() => {
  FakeTerminal.reset();
  client = new MockClient({ script: false });
  selectSession('a1');
});
afterEach(() => {
  client.close();
  selectSession(null);
});

describe('Panes', () => {
  it('shows the selected session as one headerless pane', async () => {
    await mount();
    expect(paneIds()).toEqual(['a1']);
    expect(screen.queryByRole('button', { name: 'Close pane' })).toBeNull();
    expect(screen.queryByTestId('terminal-placeholder')).toBeNull();
  });

  it('shows a placeholder while nothing is selected and nothing exists', () => {
    selectSession(null);
    render(
      <ClientProvider client={client}>
        <Panes />
      </ClientProvider>,
    );
    expect(screen.getByTestId('terminal-placeholder')).toBeInTheDocument();
  });

  it('selects the first session when none is selected yet', async () => {
    selectSession(null);
    await mount();
    expect(getSelectedSessionId()).toBe('a1');
  });

  it('follows the selection: an open session takes focus, a new one replaces the focused pane', async () => {
    await mount();
    act(() => selectSession('a4'));
    expect(paneIds()).toEqual(['a4']);
    expect(document.querySelector('[data-pane="a4"]')).toHaveAttribute('data-focused', 'true');
  });

  it('the sidebar menu\'s "Split Right" (UI command splitSession) opens that session beside the focused pane', async () => {
    await mount();
    act(() => void emitUiCommand('splitSession', 'a4'));
    expect(paneIds()).toEqual(['a1', 'a4']);
    expect(getSelectedSessionId()).toBe('a4');
  });

  it('⌥-click on a session card (data-session-id) splits, and the shared workspace store feeds the panes', async () => {
    const card = document.createElement('div');
    card.setAttribute('data-session-id', 'a3');
    document.body.append(card);
    await mount();
    fireEvent.click(card, { altKey: true });
    expect(paneIds()).toEqual(['a1', 'a3']);
    card.remove();
  });

  describe('add', () => {
    it('⌘D opens the most urgent session that is not open, beside the focused pane, and focuses it', async () => {
      await mount();
      key('KeyD');
      const ids = paneIds();
      expect(ids).toHaveLength(2);
      expect(ids[0]).toBe('a1');
      expect(getSelectedSessionId()).toBe(ids[1]);
      expect(document.querySelector(`[data-pane="${ids[1]}"]`)).toHaveAttribute('data-focused', 'true');
      expect(screen.getAllByRole('button', { name: 'Close pane' })).toHaveLength(2);
      expect(screen.getAllByTestId('pane-divider')).toHaveLength(1);
    });

    it('stops at four panes and says so', async () => {
      await mount();
      for (let i = 0; i < 3; i++) key('KeyD');
      expect(paneIds()).toHaveLength(4);
      key('KeyD');
      expect(paneIds()).toHaveLength(4);
      expect(screen.getByRole('status')).toHaveTextContent('Up to 4 panes side by side');
    });

    it('openInSplit (card context menu, palette) opens that session; asking again just focuses it', async () => {
      await mount();
      act(() => void openInSplit('a4'));
      expect(paneIds()).toEqual(['a1', 'a4']);
      act(() => selectSession('a1'));
      act(() => void openInSplit('a4'));
      expect(paneIds()).toEqual(['a1', 'a4']);
      expect(getSelectedSessionId()).toBe('a4');
    });

    it('⌥-click on a session card opens it side by side instead of selecting it', async () => {
      const onCardClick = vi.fn();
      await mount(
        <button data-session-id="a3" onClick={onCardClick}>
          card a3
        </button>,
      );
      fireEvent.click(screen.getByText('card a3'), { altKey: true });
      expect(paneIds()).toEqual(['a1', 'a3']);
      expect(onCardClick).not.toHaveBeenCalled();
      fireEvent.click(screen.getByText('card a3'));
      expect(onCardClick).toHaveBeenCalledTimes(1);
    });

    it('dropping a card on the terminal area opens it side by side, with a drop hint while dragging', async () => {
      await mount();
      const area = screen.getByTestId('panes');
      const dataTransfer = { types: [SESSION_DRAG_TYPE], getData: (t: string) => (t === SESSION_DRAG_TYPE ? 'a2' : '') };
      fireEvent.dragOver(area, { dataTransfer });
      expect(screen.getByTestId('drop-hint')).toHaveTextContent('Open side by side');
      fireEvent.drop(area, { dataTransfer });
      expect(screen.queryByTestId('drop-hint')).toBeNull();
      expect(paneIds()).toEqual(['a1', 'a2']);
    });

    it('ignores drags that are not sessions', async () => {
      await mount();
      fireEvent.dragOver(screen.getByTestId('panes'), { dataTransfer: { types: ['Files'], getData: () => '' } });
      expect(screen.queryByTestId('drop-hint')).toBeNull();
    });
  });

  describe('close', () => {
    it('⌘W closes the focused pane (the session keeps running) and selects the neighbour', async () => {
      const close = vi.spyOn(client, 'closeSession');
      await mount();
      key('KeyD');
      const second = getSelectedSessionId();
      key('KeyW');
      expect(paneIds()).toEqual(['a1']);
      expect(getSelectedSessionId()).toBe('a1');
      expect(paneIds()).not.toContain(second);
      expect(close).not.toHaveBeenCalled();
    });

    it('the pane header ✕ closes that pane', async () => {
      await mount();
      act(() => void openInSplit('a2'));
      const first = document.querySelector('[data-pane="a1"]') as HTMLElement;
      fireEvent.click(within(first).getByRole('button', { name: 'Close pane' }));
      expect(paneIds()).toEqual(['a2']);
    });

    it('the last pane cannot be closed, and ⌘W says so', async () => {
      await mount();
      key('KeyW');
      expect(paneIds()).toEqual(['a1']);
      expect(screen.getByRole('status')).toHaveTextContent('The last pane stays open');
    });

    it('closing a pane with its ✕ puts the keyboard back in the focused terminal', async () => {
      await mount();
      act(() => void openInSplit('a2'));
      await waitFor(() => expect(FakeTerminal.instances.filter((t) => !t.disposed)).toHaveLength(2));
      const focusedTerminal = FakeTerminal.last; // a2: the focused pane
      const before = focusedTerminal.focused;
      const first = document.querySelector('[data-pane="a1"]') as HTMLElement;
      fireEvent.click(within(first).getByRole('button', { name: 'Close pane' }));
      await waitFor(() => expect(focusedTerminal.focused).toBeGreaterThan(before));
      expect(focusedTerminal.disposed).toBe(false);
    });

    it('the separators are named after the panes they divide', async () => {
      await mount();
      act(() => void openInSplit('a2'));
      expect(screen.getByRole('separator')).toHaveAccessibleName('Resize panes Fix invoice rounding and Flyway migration V42');
    });

    it('a session closed in the daemon leaves its pane', async () => {
      await mount();
      act(() => void openInSplit('a2'));
      await act(() => client.closeSession('a2'));
      await waitFor(() => expect(paneIds()).toEqual(['a1']));
      expect(getSelectedSessionId()).toBe('a1');
    });
  });

  describe('tab strip focus', () => {
    it('arrowing through tabs keeps focus on the tab; clicking a tab hands the keyboard to the terminal', async () => {
      await mount(<TabStrip />);
      const tabs = await screen.findAllByRole('tab');
      (tabs[0] as HTMLElement).focus();
      fireEvent.keyDown(tabs[0] as HTMLElement, { key: 'ArrowRight' });
      await waitFor(() => expect(paneIds()).toEqual(['a2']));
      expect(document.activeElement).toBe(screen.getAllByRole('tab')[1]);
      expect(FakeTerminal.last.focused).toBe(0);

      const first = screen.getAllByRole('tab')[0] as HTMLElement;
      first.focus();
      fireEvent.click(first);
      await waitFor(() => expect(paneIds()).toEqual(['a1']));
      await waitFor(() => expect(FakeTerminal.last.focused).toBeGreaterThan(0));
    });

    it('clicking the tab of the open session also puts the keyboard in its terminal', async () => {
      await mount(<TabStrip />);
      const tab = (await screen.findAllByRole('tab'))[0] as HTMLElement;
      const textarea = document.querySelector<HTMLElement>('[data-pane="a1"] textarea');
      tab.focus();
      fireEvent.click(tab);
      expect(document.activeElement).toBe(textarea);
    });
  });

  describe('focus', () => {
    it('⌘[ and ⌘] move focus (wrapping) and select the session', async () => {
      await mount();
      act(() => void openInSplit('a2'));
      act(() => void openInSplit('a3'));
      expect(paneIds()).toEqual(['a1', 'a2', 'a3']);
      key('BracketRight');
      expect(getSelectedSessionId()).toBe('a1');
      key('BracketLeft');
      expect(getSelectedSessionId()).toBe('a3');
      key('BracketLeft');
      expect(getSelectedSessionId()).toBe('a2');
    });

    it('a pointer press on a pane focuses it; unfocused panes are dimmed', async () => {
      await mount();
      act(() => void openInSplit('a2'));
      const first = document.querySelector('[data-pane="a1"]') as HTMLElement;
      const second = document.querySelector('[data-pane="a2"]') as HTMLElement;
      expect(first.style.opacity).toBe('0.62');
      expect(second.style.opacity).toBe('1');
      fireEvent.mouseDown(first);
      expect(getSelectedSessionId()).toBe('a1');
      expect(first).toHaveAttribute('data-focused', 'true');
      expect(second.style.opacity).toBe('0.62');
    });

    it('⌘[ with a single pane does nothing', async () => {
      await mount();
      key('BracketLeft');
      expect(getSelectedSessionId()).toBe('a1');
    });

    it('shortcuts stay out of text fields that are not a terminal', async () => {
      await mount(<input aria-label="name" />);
      fireEvent.keyDown(screen.getByLabelText('name'), { code: 'KeyD', key: 'd', metaKey: true });
      expect(paneIds()).toEqual(['a1']);
    });

    it('⌘⇧D (Changes panel) does not split', async () => {
      await mount();
      key('KeyD', { shiftKey: true });
      expect(paneIds()).toEqual(['a1']);
    });
  });

  describe('resize', () => {
    const widths = () => {
      vi.spyOn(HTMLElement.prototype, 'getBoundingClientRect').mockReturnValue({ width: 500 } as DOMRect);
    };
    afterEach(() => vi.restoreAllMocks());

    it('arrow keys on a divider move it and the flex weights follow', async () => {
      await mount();
      act(() => void openInSplit('a2'));
      widths();
      const divider = screen.getByRole('separator', { name: /^Resize panes / });
      expect(divider).toHaveAttribute('aria-valuenow', '50');
      fireEvent.keyDown(divider, { key: 'ArrowRight' });
      expect(divider).toHaveAttribute('aria-valuenow', '52');
      fireEvent.keyDown(divider, { key: 'ArrowLeft' });
      fireEvent.keyDown(divider, { key: 'ArrowLeft' });
      expect(divider).toHaveAttribute('aria-valuenow', '48');
      const [a, b] = [...document.querySelectorAll<HTMLElement>('[data-pane]')];
      expect(parseFloat(a?.style.flexGrow ?? '0')).toBeLessThan(parseFloat(b?.style.flexGrow ?? '0'));
    });

    it('dragging a divider with the pointer resizes the two neighbours and stops at the minimum width', async () => {
      await mount();
      act(() => void openInSplit('a2'));
      widths();
      const divider = screen.getByRole('separator', { name: /^Resize panes / });
      fireEvent.pointerDown(divider, { clientX: 500 });
      fireEvent(window, new MouseEvent('pointermove', { clientX: 600 }));
      expect(divider).toHaveAttribute('aria-valuenow', '60');
      fireEvent(window, new MouseEvent('pointermove', { clientX: -5000 }));
      expect(divider).toHaveAttribute('aria-valuenow', '22');
      fireEvent(window, new MouseEvent('pointerup'));
      fireEvent(window, new MouseEvent('pointermove', { clientX: 700 }));
      expect(divider).toHaveAttribute('aria-valuenow', '22');
    });
  });

  describe('font size', () => {
    it('⌘+ and ⌘− change the terminal size and save it in the settings, ⌘0 resets', async () => {
      await mount();
      await waitFor(() => expect(FakeTerminal.last.options.fontSize).toBe(13));
      key('Equal', { shiftKey: true, key: '+' }); // ⌘⇧= on a US keyboard types "+"
      await waitFor(() => expect(FakeTerminal.last.options.fontSize).toBe(14));
      key('NumpadAdd');
      await waitFor(() => expect(FakeTerminal.last.options.fontSize).toBe(15));
      for (let i = 0; i < 3; i++) key('Minus');
      await waitFor(() => expect(FakeTerminal.last.options.fontSize).toBe(12));
      expect((await client.getSettings()).appearance.terminal_font_size).toBe(12);
      key('Digit0');
      await waitFor(() => expect(FakeTerminal.last.options.fontSize).toBe(13));
    });

    it('follows the typed character, not the physical key (German layout: + on BracketRight, - on Slash, [ on ⌥5)', async () => {
      await mount();
      act(() => void openInSplit('a2'));
      await waitFor(() => expect(FakeTerminal.last.options.fontSize).toBe(13));
      fireEvent.keyDown(window, { code: 'BracketRight', key: '+', metaKey: true });
      await waitFor(() => expect(FakeTerminal.last.options.fontSize).toBe(14));
      expect(getSelectedSessionId()).toBe('a2'); // not a ⌘] focus step
      fireEvent.keyDown(window, { code: 'Slash', key: '-', metaKey: true });
      await waitFor(() => expect(FakeTerminal.last.options.fontSize).toBe(13));
      fireEvent.keyDown(window, { code: 'Digit0', key: '=', metaKey: true, shiftKey: true }); // ⌘⇧0 on German types "="
      expect(FakeTerminal.last.options.fontSize).toBe(13);
      fireEvent.keyDown(window, { code: 'Digit5', key: '[', metaKey: true, altKey: true });
      expect(getSelectedSessionId()).toBe('a1');
    });

    it('stays within 9 to 32', async () => {
      await mount();
      await waitFor(() => expect(FakeTerminal.last.options.fontSize).toBe(13));
      for (let i = 0; i < 8; i++) key('Minus');
      await waitFor(() => expect(FakeTerminal.last.options.fontSize).toBe(9));
    });

    it('opens terminals with the default font when the settings cannot be read, and picks the saved font up after a reconnect', async () => {
      vi.spyOn(client, 'getSettings').mockRejectedValueOnce(new Error('daemon busy'));
      await mount();
      await waitFor(() => expect(FakeTerminal.last.options.fontSize).toBe(13));
      act(() => client.setConnection('reconnected'));
      const settings = await client.getSettings();
      await act(() => client.updateSettings({ ...settings, appearance: { ...settings.appearance, terminal_font_size: 17 } }));
      await waitFor(() => expect(FakeTerminal.last.options.fontSize).toBe(17));
    });

    it('picks up a font change made in Settings', async () => {
      await mount();
      const settings = await client.getSettings();
      await act(() =>
        client.updateSettings({ ...settings, appearance: { ...settings.appearance, terminal_font: 'Menlo', terminal_font_size: 18 } }),
      );
      await waitFor(() => expect(FakeTerminal.last.options.fontSize).toBe(18));
      expect(FakeTerminal.last.options.fontFamily).toContain('"Menlo"');
    });
  });
});
