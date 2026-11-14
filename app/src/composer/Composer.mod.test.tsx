import { act, render, screen, waitFor } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import type { MockInstance } from 'vitest';
import { ClientProvider, MockClient } from '../lib/client';
import { emitUiCommand } from '../lib/ui/commands';
import { selectSession } from '../lib/ui/selection';
import { Composer } from './Composer';
import { SUBMIT_MARKER } from './send';

let client: MockClient;
let send: MockInstance<MockClient['sendInput']>;
const writes = () => send.mock.calls.map(([id, data]) => [id, data] as [string, string]);

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

describe('Composer with the Claude mod live (source_layer mod)', () => {
  beforeEach(() => act(() => client.setSessionMeta('a1', { source_layer: 'mod' })));

  it('sends the whole prompt in one write, marked for the daemon: the mod submits it, no paste and no CR from here', async () => {
    mount();
    open();
    await userEvent.type(box(), 'run the tests{Enter}');
    await waitFor(() => expect(send).toHaveBeenCalledTimes(1));
    expect(writes()).toEqual([['a1', `${SUBMIT_MARKER}run the tests`]]);
  });

  it('keeps line breaks as they are (the daemon turns them into a paste if it has to type)', async () => {
    mount();
    open();
    await userEvent.type(box(), 'one{Shift>}{Enter}{/Shift}two{Enter}');
    await waitFor(() => expect(send).toHaveBeenCalledTimes(1));
    expect(writes()).toEqual([['a1', `${SUBMIT_MARKER}one\ntwo`]]);
  });

  it('cannot be tricked into a second marker: control characters in the text are dropped', async () => {
    mount();
    open();
    await userEvent.click(box());
    await userEvent.paste(`a\u0000lotse-submit\u0000b`);
    await userEvent.keyboard('{Enter}');
    await waitFor(() => expect(send).toHaveBeenCalledTimes(1));
    const [, data] = writes()[0]!;
    expect(data.startsWith(SUBMIT_MARKER)).toBe(true);
    expect(data.slice(SUBMIT_MARKER.length)).not.toContain('\u0000');
  });

  it('a failed write puts the words back and says why, like the typed send', async () => {
    send.mockRejectedValueOnce(new Error('daemon is gone'));
    mount();
    open();
    await userEvent.type(box(), 'keep me{Enter}');
    expect(await screen.findByRole('alert')).toHaveTextContent('Not sent: daemon is gone');
    expect(box()).toHaveValue('keep me');
  });
});

describe('Composer without the mod', () => {
  it.each(['hooks', 'screen', undefined] as const)('source_layer %s: the paste and the CR as before, never the marker', async (layer) => {
    act(() => client.setSessionMeta('a1', { source_layer: layer }));
    mount();
    open();
    await userEvent.type(box(), 'go{Enter}');
    await waitFor(() => expect(send).toHaveBeenCalledTimes(2));
    expect(writes()).toEqual([
      ['a1', 'go'],
      ['a1', '\r'],
    ]);
  });
});
