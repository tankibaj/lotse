import { act, render, screen, waitFor } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import type { MockInstance } from 'vitest';
import { ClientProvider, MockClient } from '../lib/client';
import { emitUiCommand } from '../lib/ui/commands';
import { selectSession } from '../lib/ui/selection';
import { registerTerminal } from '../terminal/registry';
import { Composer } from './Composer';

let client: MockClient;
let send: MockInstance<MockClient['sendInput']>;
let forget: (() => void) | null = null;
/** What went to `sendInput`, in order. */
const writes = () => send.mock.calls.map(([id, data]) => [id, data] as [string, string]);
const wrote = (n: number) => waitFor(() => expect(send).toHaveBeenCalledTimes(n));

const mount = () =>
  render(
    <ClientProvider client={client}>
      <Composer />
    </ClientProvider>,
  );
const open = () => act(() => void emitUiCommand('openComposer'));
const box = () => screen.getByRole('textbox', { name: 'Prompt composer' });

beforeEach(() => {
  client = new MockClient({ script: false });
  send = vi.spyOn(client, 'sendInput');
  selectSession('a1');
});
afterEach(() => {
  forget?.();
  forget = null;
  selectSession(null);
});

describe('Composer', () => {
  it('stays hidden until openComposer, then focuses the textarea', () => {
    mount();
    expect(screen.queryByRole('textbox')).toBeNull();
    open();
    expect(box()).toHaveFocus();
  });

  it('does not open without a selected session', () => {
    selectSession(null);
    mount();
    open();
    expect(screen.queryByRole('textbox')).toBeNull();
  });

  it('return_sends_to_focused_session', async () => {
    mount();
    open();
    await userEvent.type(box(), 'run the tests{Enter}');
    expect(screen.queryByRole('textbox')).toBeNull(); // sent: closed
    await wrote(2);
    expect(writes()).toEqual([
      ['a1', 'run the tests'],
      ['a1', '\r'],
    ]);
  });

  it('return_sends_to_the_session_selected_at_that_moment', async () => {
    mount();
    open();
    act(() => selectSession('a2'));
    await userEvent.type(box(), 'hi{Enter}');
    await wrote(2);
    expect(writes()).toEqual([
      ['a2', 'hi'],
      ['a2', '\r'],
    ]);
  });

  describe('the bytes of a send', () => {
    const typeTwoLines = async () => {
      mount();
      open();
      await userEvent.type(box(), 'echo light-1{Shift>}{Enter}{/Shift}echo light-2{Enter}');
      await wrote(2);
    };

    it('bracketed paste off: the text with each line break as CR (what Terminal.paste writes), then the submitting CR', async () => {
      await typeTwoLines();
      expect(writes()).toEqual([
        ['a1', 'echo light-1\recho light-2'],
        ['a1', '\r'],
      ]);
    });

    it('bracketed paste on: the text wrapped in ESC[200~ … ESC[201~, then the submitting CR', async () => {
      forget = registerTerminal('a1', { modes: { bracketedPasteMode: true } });
      await typeTwoLines();
      expect(writes()).toEqual([
        ['a1', '\x1b[200~echo light-1\recho light-2\x1b[201~'],
        ['a1', '\r'],
      ]);
    });

    it('a single line is a paste too, and no bare LF ever goes out', async () => {
      forget = registerTerminal('a1', { modes: { bracketedPasteMode: true } });
      mount();
      open();
      await userEvent.type(box(), 'ls -la{Enter}');
      await wrote(2);
      expect(writes()).toEqual([
        ['a1', '\x1b[200~ls -la\x1b[201~'],
        ['a1', '\r'],
      ]);
      expect(writes().every(([, data]) => !data.includes('\n'))).toBe(true);
    });

    it('the CR is a separate write, at least 150 ms after the text', async () => {
      mount();
      open();
      const at: number[] = [];
      send.mockImplementation(async () => void at.push(Date.now()));
      await userEvent.type(box(), 'go{Enter}');
      await wrote(2);
      expect(at[1]! - at[0]!).toBeGreaterThanOrEqual(140); // timers may fire a few ms early
    });

    it('pasted text keeps its own trailing line breaks out: one submit only', async () => {
      mount();
      open();
      await userEvent.type(box(), 'one{Shift>}{Enter}{Enter}{/Shift}{Enter}');
      await wrote(2);
      expect(writes()).toEqual([
        ['a1', 'one'],
        ['a1', '\r'],
      ]);
    });

    it('only asks the terminal of the session it sends to', async () => {
      forget = registerTerminal('a2', { modes: { bracketedPasteMode: true } });
      mount();
      open();
      await userEvent.type(box(), 'x{Enter}'); // a1 has no bracketed-paste terminal
      await wrote(2);
      expect(writes()[0]).toEqual(['a1', 'x']);
    });
  });

  it('a failed write puts the words back, reopens the box and says why', async () => {
    send.mockRejectedValueOnce(new Error('daemon is gone'));
    mount();
    open();
    await userEvent.type(box(), 'keep me{Enter}');
    expect(await screen.findByRole('alert')).toHaveTextContent('Not sent: daemon is gone');
    expect(box()).toHaveValue('keep me');
    expect(send).toHaveBeenCalledTimes(1); // no CR after a text that never went out
  });

  describe('layout', () => {
    it('sits in the flow below the panes (the terminal shrinks to make room), not over them', () => {
      mount();
      open();
      const card = screen.getByTestId('composer');
      expect(card).not.toHaveClass('absolute');
      expect(card).toHaveClass('shrink-0');
    });

    it('draws the focus ring on the card, not as a square halo inside it', () => {
      mount();
      open();
      expect(screen.getByTestId('composer').className).toMatch(/focus-within:/);
      expect(box().className).toMatch(/outline-none!/);
    });

    it('names the session it sends to once more than one pane is visible', () => {
      const panes = ['a1', 'a2'].map((id) => {
        const el = document.createElement('div');
        el.setAttribute('data-pane', id);
        document.body.append(el);
        return el;
      });
      mount();
      open();
      return waitFor(() => expect(screen.getByTestId('composer')).toHaveTextContent('Send to Fix invoice rounding')).finally(() => panes.forEach((p) => p.remove()));
    });

    it('does not name the session with a single pane', () => {
      const pane = document.createElement('div');
      pane.setAttribute('data-pane', 'a1');
      document.body.append(pane);
      mount();
      open();
      expect(screen.getByTestId('composer')).not.toHaveTextContent('Send to');
      pane.remove();
    });
  });

  it('shift_return_newline', async () => {
    mount();
    open();
    await userEvent.type(box(), 'one{Shift>}{Enter}{/Shift}two');
    expect(send).not.toHaveBeenCalled();
    expect(box()).toHaveValue('one\ntwo');
  });

  it('empty_return_sends_nothing', async () => {
    mount();
    open();
    await userEvent.type(box(), '{Enter}');
    await userEvent.type(box(), '  {Shift>}{Enter}{/Shift} {Enter}');
    expect(send).not.toHaveBeenCalled();
    expect(box()).toBeInTheDocument(); // still open
  });

  it('Esc closes and keeps the draft of that session', async () => {
    mount();
    open();
    await userEvent.type(box(), 'half a thought{Escape}');
    expect(screen.queryByRole('textbox')).toBeNull();
    expect(send).not.toHaveBeenCalled();
    open();
    expect(box()).toHaveValue('half a thought');
  });

  it('keeps one draft per session', async () => {
    mount();
    open();
    await userEvent.type(box(), 'for a1');
    act(() => selectSession('a2'));
    expect(box()).toHaveValue('');
    await userEvent.type(box(), 'for a2');
    act(() => selectSession('a1'));
    expect(box()).toHaveValue('for a1');
    act(() => selectSession('a2'));
    expect(box()).toHaveValue('for a2');
  });

  it('a sent draft is gone', async () => {
    mount();
    open();
    await userEvent.type(box(), 'once{Enter}');
    open();
    expect(box()).toHaveValue('');
  });

  it('does not send while an IME composition is confirmed with Return', async () => {
    mount();
    open();
    await userEvent.type(box(), 'x');
    const ta = box();
    const ev = new KeyboardEvent('keydown', { key: 'Enter', bubbles: true, isComposing: true });
    act(() => void ta.dispatchEvent(ev));
    expect(send).not.toHaveBeenCalled();
  });

  it('gives focus back to what had it before it opened', async () => {
    const outside = document.createElement('button');
    document.body.append(outside);
    outside.focus();
    mount();
    open();
    expect(box()).toHaveFocus();
    await userEvent.keyboard('{Escape}');
    expect(outside).toHaveFocus();
    outside.remove();
  });
});
