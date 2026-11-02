import { render, screen } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { ClientProvider, MockClient } from '../lib/client';
import { FakeTerminal } from './fakeXterm';
import { moveFocusOut } from './focus';
import { TerminalView } from './TerminalView';

vi.mock('@xterm/xterm', async () => (await import('./fakeXterm')).xtermModule);
vi.mock('@xterm/addon-fit', async () => (await import('./fakeXterm')).fitModule);
vi.mock('@xterm/addon-web-links', async () => (await import('./fakeXterm')).linksModule);
vi.mock('@xterm/addon-webgl', async () => (await import('./fakeXterm')).webglModule);

describe('moveFocusOut', () => {
  it('focuses the next and previous control in page order, wrapping, and skips terminals', () => {
    render(
      <>
        <button>first</button>
        <div className="xterm">
          <textarea aria-label="term" />
        </div>
        <button>second</button>
        <button tabIndex={-1}>roving</button>
        <button disabled>off</button>
        <div hidden>
          <button>hidden</button>
        </div>
        <button>last</button>
      </>,
    );
    const term = screen.getByLabelText('term');
    expect(moveFocusOut(term, 1)).toBe(true);
    expect(document.activeElement).toBe(screen.getByText('second'));
    expect(moveFocusOut(term, -1)).toBe(true);
    expect(document.activeElement).toBe(screen.getByText('first'));
    expect(moveFocusOut(screen.getByText('last'), 1)).toBe(true);
    expect(document.activeElement).toBe(screen.getByText('first'));
    expect(moveFocusOut(screen.getByText('first'), -1)).toBe(true);
    expect(document.activeElement).toBe(screen.getByText('last'));
  });

  it('reports false when there is nothing to focus', () => {
    render(
      <div className="xterm">
        <textarea aria-label="term" />
      </div>,
    );
    expect(moveFocusOut(screen.getByLabelText('term'), 1)).toBe(false);
  });
});

describe('TerminalView keyboard exit', () => {
  let client: MockClient;
  beforeEach(() => {
    FakeTerminal.reset();
    client = new MockClient({ script: false });
  });
  afterEach(() => client.close());

  it('Ctrl+Tab and Ctrl+Shift+Tab leave the terminal instead of sending Tab to the shell; plain Tab stays with the shell', () => {
    render(
      <ClientProvider client={client}>
        <button>before</button>
        <TerminalView sessionId="a1" />
        <button>after</button>
      </ClientProvider>,
    );
    const handle = FakeTerminal.last.keyHandler;
    const key = (init: KeyboardEventInit) => handle?.(new KeyboardEvent('keydown', { key: 'Tab', cancelable: true, ...init }));
    expect(key({})).toBe(true);
    expect(key({ shiftKey: true })).toBe(true);
    expect(key({ ctrlKey: true })).toBe(false);
    expect(document.activeElement).toBe(screen.getByText('after'));
    expect(key({ ctrlKey: true, shiftKey: true })).toBe(false);
    expect(document.activeElement).toBe(screen.getByText('before'));
  });

  it('describes the exit key to assistive technology', () => {
    render(
      <ClientProvider client={client}>
        <TerminalView sessionId="a1" />
      </ClientProvider>,
    );
    expect(screen.getByRole('group', { name: 'Terminal' })).toHaveAccessibleDescription('Control+Tab moves focus out of the terminal');
  });
});
