import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { NotSubmitted } from '../composer/send';
import { sendReviewPrompt } from './send';

// No terminal is mounted in these tests: `bracketedPasteOn` is false for every session.
let writes: string[];
const client = {
  sendInput: vi.fn(async (_id: string, text: string) => {
    writes.push(text);
  }),
};

beforeEach(() => {
  vi.useFakeTimers();
  writes = [];
  client.sendInput.mockClear();
});
afterEach(() => vi.useRealTimers());

describe('sendReviewPrompt', () => {
  it('is one bracketed paste even when the terminal is not mounted, then one Return', async () => {
    const done = sendReviewPrompt(client, 's1', 'one\n\ntwo\nthree');
    await vi.runAllTimersAsync();
    await done;
    expect(writes).toEqual(['\x1b[200~one\r\rtwo\rthree\x1b[201~', '\r']);
  });

  it('a pasted ESC[201~ cannot end the paste early', async () => {
    const done = sendReviewPrompt(client, 's1', 'a\x1b[201~rm -rf /');
    await vi.runAllTimersAsync();
    await done;
    expect(writes[0]).toBe('\x1b[200~a[201~rm -rf /\x1b[201~');
    expect(writes[0]?.split('\x1b[201~')).toHaveLength(2);
  });

  it('a failed paste rejects (comments stay), a failed Return is NotSubmitted', async () => {
    client.sendInput.mockRejectedValueOnce(new Error('daemon gone'));
    await expect(sendReviewPrompt(client, 's1', 'x')).rejects.not.toBeInstanceOf(NotSubmitted);
    client.sendInput.mockResolvedValueOnce(undefined).mockRejectedValueOnce(new Error('late'));
    const done = sendReviewPrompt(client, 's1', 'x');
    const check = expect(done).rejects.toBeInstanceOf(NotSubmitted);
    await vi.runAllTimersAsync();
    await check;
  });
});
