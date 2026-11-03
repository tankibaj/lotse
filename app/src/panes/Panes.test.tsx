import { act, createEvent, fireEvent, render, screen, waitFor, within } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { ClientProvider, MockClient } from '../lib/client';
import { emitUiCommand } from '../lib/ui/commands';
import { getSelectedSessionId, selectSession } from '../lib/ui/selection';
import { FakeTerminal } from '../terminal/fakeXterm';
import { Panes, SESSION_DRAG_TYPE } from './Panes';
import { dragTabOverPanes, openInSplit } from './splitBus';
import type { TabDrag } from './splitBus';
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

    it('⇧⌘R (full diff review) and ⌘⌥D do not split', async () => {
      await mount();
      fireEvent.keyDown(window, { code: 'KeyR', key: 'R', metaKey: true, shiftKey: true });
      key('KeyD', { altKey: true });
      expect(paneIds()).toEqual(['a1']);
    });

    it('⌘[ and ⌘] walk a stacked column top to bottom, then on to the next column', async () => {
      await mount();
      act(() => void openInSplit('a2', 'down'));
      act(() => void openInSplit('a3', 'right'));
      expect(paneIds()).toEqual(['a1', 'a2', 'a3']);
      expect(getSelectedSessionId()).toBe('a3');
      key('BracketRight');
      expect(getSelectedSessionId()).toBe('a1');
      key('BracketRight');
      expect(getSelectedSessionId()).toBe('a2'); // the pane below a1
      key('BracketRight');
      expect(getSelectedSessionId()).toBe('a3');
      key('BracketLeft');
      expect(getSelectedSessionId()).toBe('a2');
    });

    it('a pointer press on a stacked pane focuses it', async () => {
      await mount();
      act(() => void openInSplit('a2', 'down'));
      fireEvent.mouseDown(document.querySelector('[data-pane="a1"]') as HTMLElement);
      expect(getSelectedSessionId()).toBe('a1');
      expect(document.querySelector('[data-pane="a1"]')).toHaveAttribute('data-focused', 'true');
      expect((document.querySelector('[data-pane="a2"]') as HTMLElement).style.opacity).toBe('0.62');
    });
  });

  describe('split down', () => {
    const columns = () => [...document.querySelectorAll('[data-column]')].map((c) => [...c.querySelectorAll('[data-pane]')].map((p) => p.getAttribute('data-pane')));

    it('⌘⇧D opens the most urgent session that is not open below the focused pane, and focuses it', async () => {
      await mount();
      key('KeyD', { shiftKey: true, key: 'D' });
      const ids = paneIds();
      expect(ids).toHaveLength(2);
      expect(columns()).toEqual([ids]);
      expect(getSelectedSessionId()).toBe(ids[1]);
      expect(document.querySelector(`[data-pane="${ids[1]}"]`)).toHaveAttribute('data-focused', 'true');
      expect(screen.getAllByRole('button', { name: 'Close pane' })).toHaveLength(2);
      const divider = screen.getByTestId('pane-stack-divider');
      expect(divider).toHaveAttribute('aria-orientation', 'horizontal');
      expect(divider.className).toContain('cursor-row-resize');
      expect(screen.queryByTestId('pane-divider')).toBeNull();
    });

    it('⌘⇧D picks the session the same way ⌘D does', async () => {
      await mount();
      key('KeyD');
      const right = paneIds()[1];
      key('KeyW');
      key('KeyD', { shiftKey: true, key: 'D' });
      expect(paneIds()[1]).toBe(right);
    });

    it('right and down mix: a column beside a column of two', async () => {
      await mount();
      act(() => void openInSplit('a2', 'right'));
      act(() => void openInSplit('a3', 'down'));
      expect(columns()).toEqual([['a1'], ['a2', 'a3']]);
      expect(screen.getAllByTestId('pane-divider')).toHaveLength(1);
      expect(screen.getAllByTestId('pane-stack-divider')).toHaveLength(1);
      expect(screen.getAllByRole('separator')).toHaveLength(2);
    });

    it('the pane limit counts every pane, whichever way it was split', async () => {
      await mount();
      act(() => void openInSplit('a2', 'down'));
      act(() => void openInSplit('a3', 'right'));
      act(() => void openInSplit('a4', 'down'));
      expect(paneIds()).toHaveLength(4);
      key('KeyD', { shiftKey: true, key: 'D' });
      key('KeyD');
      expect(paneIds()).toHaveLength(4);
      expect(screen.getByRole('status')).toHaveTextContent('Up to 4 panes side by side');
    });

    it('the sidebar and tab menus\' "Split Down" (UI command splitSessionDown) opens that session below', async () => {
      await mount();
      act(() => void emitUiCommand('splitSessionDown', 'a4'));
      expect(columns()).toEqual([['a1', 'a4']]);
      expect(getSelectedSessionId()).toBe('a4');
      act(() => void emitUiCommand('splitSession', 'a3'));
      expect(columns()).toEqual([['a1', 'a4'], ['a3']]);
    });

    it('⌥⇧-click on a session card splits down; ⌥-click still splits right', async () => {
      const onCardClick = vi.fn();
      await mount(
        <>
          <button data-session-id="a3" onClick={onCardClick}>card a3</button>
          <button data-session-id="a4" onClick={onCardClick}>card a4</button>
        </>,
      );
      fireEvent.click(screen.getByText('card a3'), { altKey: true, shiftKey: true });
      expect(columns()).toEqual([['a1', 'a3']]);
      fireEvent.click(screen.getByText('card a4'), { altKey: true });
      expect(columns()).toEqual([['a1', 'a3'], ['a4']]);
      expect(onCardClick).not.toHaveBeenCalled();
    });

    it('closing a stacked pane collapses its split and the survivor takes the whole column', async () => {
      await mount();
      act(() => void openInSplit('a2', 'down'));
      key('KeyW');
      expect(columns()).toEqual([['a1']]);
      expect(screen.queryByTestId('pane-stack-divider')).toBeNull();
      expect(getSelectedSessionId()).toBe('a1');
    });

    it('closing a pane with its ✕ in a stack keeps the other terminals alive', async () => {
      await mount();
      act(() => void openInSplit('a2', 'down'));
      act(() => void openInSplit('a3', 'right'));
      await waitFor(() => expect(FakeTerminal.instances.filter((t) => !t.disposed)).toHaveLength(3));
      const second = document.querySelector('[data-pane="a2"]') as HTMLElement;
      fireEvent.click(within(second).getByRole('button', { name: 'Close pane' }));
      expect(columns()).toEqual([['a1'], ['a3']]);
      expect(FakeTerminal.instances.filter((t) => !t.disposed)).toHaveLength(2);
    });

    it('splitting does not restart the terminals that are already open', async () => {
      await mount();
      act(() => void openInSplit('a2', 'right'));
      await waitFor(() => expect(FakeTerminal.instances.filter((t) => !t.disposed)).toHaveLength(2));
      const before = FakeTerminal.instances.length;
      act(() => void openInSplit('a3', 'down'));
      await waitFor(() => expect(FakeTerminal.instances.filter((t) => !t.disposed)).toHaveLength(3));
      expect(FakeTerminal.instances).toHaveLength(before + 1);
    });

    it('a stacked terminal that refits tells the daemon its new size (the existing resize call)', async () => {
      const resize = vi.spyOn(client, 'resize');
      await mount();
      act(() => void openInSplit('a2', 'down'));
      await waitFor(() => expect(FakeTerminal.instances.filter((t) => !t.disposed)).toHaveLength(2));
      await waitFor(() => {
        FakeTerminal.last.resizeTo(100, 12);
        expect(resize).toHaveBeenCalledWith('a2', 100, 12);
      });
    });
  });

  describe('drop zones', () => {
    const rect = (left: number, top: number, width: number, height: number) => ({ left, top, width, height, right: left + width, bottom: top + height, x: left, y: top, toJSON: () => ({}) }) as DOMRect;
    const dataTransfer = (id = 'a2') => ({ types: [SESSION_DRAG_TYPE], getData: (t: string) => (t === SESSION_DRAG_TYPE ? id : '') });
    /** jsdom has no DragEvent coordinates: put them on the event. */
    const drag = (type: 'dragOver' | 'drop', target: Element, x: number, y: number, id?: string) => {
      const ev = createEvent[type](target, { dataTransfer: dataTransfer(id) });
      Object.defineProperties(ev, { clientX: { value: x }, clientY: { value: y } });
      fireEvent(target, ev);
    };
    /** The panes area is 1000 x 800 at the origin; the first pane fills it. */
    const layoutBox = () => {
      (screen.getByTestId('panes') as HTMLElement).getBoundingClientRect = () => rect(0, 0, 1000, 800);
      document.querySelectorAll<HTMLElement>('[data-pane]').forEach((p) => (p.getBoundingClientRect = () => rect(0, 0, 1000, 800)));
    };
    const hint = () => screen.queryByTestId('drop-hint');

    it('over the bottom half of a pane the zone is the bottom half and says "Open below"; dropping stacks it', async () => {
      await mount();
      layoutBox();
      const pane = document.querySelector('[data-pane="a1"]') as HTMLElement;
      drag('dragOver', pane, 300, 700);
      expect(hint()).toHaveTextContent('Open below');
      expect(hint()).toHaveAttribute('data-zone', 'down');
      expect(hint()).toHaveStyle({ left: '12px', top: '412px', width: '976px', height: '376px' });
      drag('drop', pane, 300, 700);
      expect(hint()).toBeNull();
      expect(paneIds()).toEqual(['a1', 'a2']);
      expect(document.querySelectorAll('[data-column]')).toHaveLength(1);
      expect(getSelectedSessionId()).toBe('a2');
    });

    it('over the right half the zone is the right half and says "Open side by side"; dropping splits right', async () => {
      await mount();
      layoutBox();
      const pane = document.querySelector('[data-pane="a1"]') as HTMLElement;
      drag('dragOver', pane, 800, 200);
      expect(hint()).toHaveTextContent('Open side by side');
      expect(hint()).toHaveAttribute('data-zone', 'right');
      expect(hint()).toHaveStyle({ left: '512px', top: '12px', width: '476px', height: '776px' });
      drag('drop', pane, 800, 200);
      expect(document.querySelectorAll('[data-column]')).toHaveLength(2);
    });

    it('the zone follows the pointer from the right half to the bottom half', async () => {
      await mount();
      layoutBox();
      const pane = document.querySelector('[data-pane="a1"]') as HTMLElement;
      drag('dragOver', pane, 800, 200);
      expect(hint()).toHaveAttribute('data-zone', 'right');
      drag('dragOver', pane, 200, 700);
      expect(hint()).toHaveAttribute('data-zone', 'down');
    });

    it('a drop splits the pane it lands on, not the focused one', async () => {
      await mount();
      act(() => void openInSplit('a2', 'right'));
      act(() => void openInSplit('a3', 'right')); // focus is on a3
      const sections = [...document.querySelectorAll<HTMLElement>('[data-pane]')];
      (screen.getByTestId('panes') as HTMLElement).getBoundingClientRect = () => rect(0, 0, 900, 800);
      sections.forEach((p, i) => (p.getBoundingClientRect = () => rect(i * 300, 0, 300, 800)));
      drag('drop', document.querySelector('[data-pane="a1"]') as HTMLElement, 100, 700, 'a4'); // bottom of the first pane
      expect([...document.querySelectorAll('[data-column]')].map((c) => [...c.querySelectorAll('[data-pane]')].map((p) => p.getAttribute('data-pane')))).toEqual([['a1', 'a4'], ['a2'], ['a3']]);
    });

    it('dragging away from the panes hides the zone, and a drag that is not a session never shows one', async () => {
      await mount();
      layoutBox();
      const area = screen.getByTestId('panes');
      drag('dragOver', document.querySelector('[data-pane="a1"]') as HTMLElement, 800, 200);
      expect(hint()).not.toBeNull();
      fireEvent.dragLeave(area, { relatedTarget: document.body });
      expect(hint()).toBeNull();
      fireEvent.dragOver(area, { dataTransfer: { types: ['Files'], getData: () => '' } });
      expect(hint()).toBeNull();
    });

    it('a full layout says so instead of splitting', async () => {
      await mount();
      for (const id of ['a2', 'a3', 'a4']) act(() => void openInSplit(id, 'right'));
      layoutBox();
      drag('drop', document.querySelector('[data-pane="a4"]') as HTMLElement, 100, 700, 'a5');
      expect(paneIds()).toHaveLength(4);
      expect(screen.getByRole('status')).toHaveTextContent('Up to 4 panes side by side');
    });

    describe('a tab dragged from the tab strip', () => {
      const tabDrag = (d: TabDrag) => {
        let over = false;
        act(() => {
          over = dragTabOverPanes(d);
        });
        return over;
      };

      it('is over the panes only where a pane is; the zone shows there and goes when the pointer leaves', async () => {
        await mount();
        layoutBox();
        expect(tabDrag({ phase: 'move', sessionId: 'a2', x: 300, y: 700 })).toBe(true);
        expect(hint()).toHaveAttribute('data-zone', 'down');
        expect(tabDrag({ phase: 'move', sessionId: 'a2', x: 300, y: -20 })).toBe(false);
        expect(hint()).toBeNull();
        tabDrag({ phase: 'move', sessionId: 'a2', x: 900, y: 100 });
        expect(hint()).toHaveAttribute('data-zone', 'right');
        tabDrag({ phase: 'cancel' });
        expect(hint()).toBeNull();
      });

      it('dropped on the bottom half stacks it, and undoes the swap the mouse-down on the tab made', async () => {
        await mount();
        act(() => selectSession('a2')); // mouse down on the a2 tab: a2 takes the focused pane's place
        expect(paneIds()).toEqual(['a2']);
        layoutBox();
        expect(tabDrag({ phase: 'drop', sessionId: 'a2', x: 300, y: 700, restore: 'a1' })).toBe(true);
        expect(paneIds()).toEqual(['a1', 'a2']);
        expect(document.querySelectorAll('[data-column]')).toHaveLength(1);
        expect(getSelectedSessionId()).toBe('a2');
        expect(hint()).toBeNull();
      });

      it('dropped on the right half opens a column', async () => {
        await mount();
        act(() => selectSession('a2'));
        layoutBox();
        tabDrag({ phase: 'drop', sessionId: 'a2', x: 900, y: 100, restore: 'a1' });
        expect(paneIds()).toEqual(['a1', 'a2']);
        expect(document.querySelectorAll('[data-column]')).toHaveLength(2);
      });

      it('a tab whose session is already open just focuses it', async () => {
        await mount();
        act(() => void openInSplit('a2', 'right'));
        act(() => selectSession('a1')); // pressing the a1 tab: it is open already, so only the focus moves
        layoutBox();
        tabDrag({ phase: 'drop', sessionId: 'a1', x: 300, y: 700, restore: 'a2' });
        expect(paneIds()).toEqual(['a1', 'a2']);
        expect(getSelectedSessionId()).toBe('a1');
      });
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
      const [a, b] = [...document.querySelectorAll<HTMLElement>('[data-column]')]; // a column's width is its own flex weight
      expect(parseFloat(a?.style.flexGrow ?? '0')).toBeLessThan(parseFloat(b?.style.flexGrow ?? '0'));
    });

    const sized = () => {
      vi.spyOn(HTMLElement.prototype, 'getBoundingClientRect').mockReturnValue({ width: 500, height: 300 } as DOMRect);
    };

    it('arrow keys on the divider between stacked panes move it (up and down) and the heights follow', async () => {
      await mount();
      act(() => void openInSplit('a2', 'down'));
      sized();
      const divider = screen.getByTestId('pane-stack-divider');
      expect(divider).toHaveAccessibleName('Resize panes Fix invoice rounding and Flyway migration V42');
      expect(divider).toHaveAttribute('aria-valuenow', '50');
      fireEvent.keyDown(divider, { key: 'ArrowDown' });
      expect(divider).toHaveAttribute('aria-valuenow', '54'); // 24 px of the 600 px pair
      fireEvent.keyDown(divider, { key: 'ArrowUp' });
      fireEvent.keyDown(divider, { key: 'ArrowUp' });
      expect(divider).toHaveAttribute('aria-valuenow', '46');
      fireEvent.keyDown(divider, { key: 'ArrowLeft' }); // sideways keys belong to the vertical dividers
      expect(divider).toHaveAttribute('aria-valuenow', '46');
      const [a, b] = [...document.querySelectorAll<HTMLElement>('[data-pane]')];
      expect(parseFloat(a?.style.flexGrow ?? '0')).toBeLessThan(parseFloat(b?.style.flexGrow ?? '0'));
    });

    it('dragging the divider between stacked panes with the pointer resizes the two panes and stops at the minimum height', async () => {
      await mount();
      act(() => void openInSplit('a2', 'down'));
      sized();
      const divider = screen.getByTestId('pane-stack-divider');
      fireEvent.pointerDown(divider, { clientY: 300 });
      expect(document.body.style.cursor).toBe('row-resize');
      fireEvent(window, new MouseEvent('pointermove', { clientY: 360 }));
      expect(divider).toHaveAttribute('aria-valuenow', '60'); // 360 px of the 600 px pair
      fireEvent(window, new MouseEvent('pointermove', { clientY: -5000 }));
      expect(divider).toHaveAttribute('aria-valuenow', '20'); // 120 px
      fireEvent(window, new MouseEvent('pointermove', { clientY: 5000 }));
      expect(divider).toHaveAttribute('aria-valuenow', '80');
      fireEvent(window, new MouseEvent('pointerup'));
      expect(document.body.style.cursor).toBe('');
      fireEvent(window, new MouseEvent('pointermove', { clientY: 300 }));
      expect(divider).toHaveAttribute('aria-valuenow', '80');
    });

    it('in a mixed layout each divider resizes its own axis', async () => {
      await mount();
      act(() => void openInSplit('a2', 'right'));
      act(() => void openInSplit('a3', 'down'));
      sized();
      const across = screen.getByTestId('pane-divider');
      const stack = screen.getByTestId('pane-stack-divider');
      fireEvent.keyDown(across, { key: 'ArrowRight' });
      expect(across).toHaveAttribute('aria-valuenow', '52'); // 24 px of the 1000 px pair
      expect(stack).toHaveAttribute('aria-valuenow', '50');
      fireEvent.keyDown(stack, { key: 'ArrowDown' });
      expect(stack).toHaveAttribute('aria-valuenow', '54'); // 24 px of the 600 px pair
      expect(across).toHaveAttribute('aria-valuenow', '52');
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
