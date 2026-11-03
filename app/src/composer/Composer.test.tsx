import { act, render, screen } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { ClientProvider, MockClient } from '../lib/client';
import { emitUiCommand } from '../lib/ui/commands';
import { selectSession } from '../lib/ui/selection';
import { Composer } from './Composer';

let client: MockClient;
let send: ReturnType<typeof vi.spyOn>;

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
afterEach(() => selectSession(null));

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
    expect(send).toHaveBeenCalledTimes(1);
    expect(send).toHaveBeenCalledWith('a1', 'run the tests\r');
    expect(screen.queryByRole('textbox')).toBeNull(); // sent: closed
  });

  it('return_sends_to_the_session_selected_at_that_moment', async () => {
    mount();
    open();
    act(() => selectSession('a2'));
    await userEvent.type(box(), 'hi{Enter}');
    expect(send).toHaveBeenCalledWith('a2', 'hi\r');
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
