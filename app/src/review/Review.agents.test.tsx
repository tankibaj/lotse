import { cleanup, configure, render, screen, waitFor, within } from '@testing-library/react';
import { afterEach, beforeAll, beforeEach, describe, expect, it, vi } from 'vitest';
import { ClientProvider, MockClient } from '../lib/client';
import type { DiffFile, Transcript, TranscriptTurn } from '../lib/client/types';
import { Review } from './index';

// Show Changes: which sub-agent changed a file, from the transcript's Edit/Write turns. No transcript, no label.

const file = (path: string): DiffFile => ({ path, old_path: null, status: 'modified', binary: false, truncated: false, diff: '', added: 1, removed: 0 });
const turn = (summary: string, sub_agent?: string): TranscriptTurn => ({ role: 'tool', text: '', tool: { name: 'Edit', summary }, at: null, ...(sub_agent ? { sub_agent } : {}) });
const transcript = (turns: TranscriptTurn[]): Transcript => ({ available: true, turns, truncated: false });

let client: MockClient;
beforeAll(() => {
  configure({ getElementError: (message) => new Error(message ?? '') });
  const rects = { length: 0, item: () => null, [Symbol.iterator]: [][Symbol.iterator] } as unknown as DOMRectList;
  Range.prototype.getClientRects = () => rects;
  Range.prototype.getBoundingClientRect = () => new DOMRect();
});
beforeEach(() => {
  client = new MockClient({ script: false });
  vi.spyOn(client, 'getDiff').mockResolvedValue({ is_git: true, files: [file('src/a.ts'), file('src/b.ts')], truncated: false });
});
afterEach(() => {
  cleanup();
  client.close();
});

const mount = () =>
  render(
    <ClientProvider client={client}>
      <Review sessionId="a1" onClose={() => {}} />
    </ClientProvider>,
  );
const row = (name: string) => within(screen.getByRole('complementary', { name: 'Changed files' })).getByRole('button', { name: new RegExp(name) });

describe('per-agent changes', () => {
  it('labels a file with the sub-agent that edited it', async () => {
    vi.spyOn(client, 'getTranscript').mockResolvedValue(transcript([turn('src/a.ts', 'lint-fixer'), turn('src/b.ts')]));
    mount();
    await waitFor(() => expect(row('a.ts').querySelector('[data-part="by-agent"]')).toHaveTextContent('lint-fixer'));
    expect(row('b.ts').querySelector('[data-part="by-agent"]')).toBeNull();
  });

  it('shows no label when the transcript is unavailable or fails', async () => {
    vi.spyOn(client, 'getTranscript').mockRejectedValue(new Error('no transcript'));
    mount();
    await screen.findByRole('button', { name: /a\.ts/ });
    expect(document.querySelector('[data-part="by-agent"]')).toBeNull();
  });
});
