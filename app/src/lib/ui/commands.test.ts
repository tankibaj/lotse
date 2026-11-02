import { describe, expect, it, vi } from 'vitest';
import { emitUiCommand, onUiCommand } from './commands';

describe('ui commands', () => {
  it('calls subscribers, reports the count, and unsubscribes', () => {
    const h = vi.fn();
    expect(emitUiCommand('openPalette')).toBe(0);
    const off = onUiCommand('openPalette', h);
    expect(emitUiCommand('openPalette')).toBe(1);
    expect(h).toHaveBeenCalledTimes(1);
    off();
    expect(emitUiCommand('openPalette')).toBe(0);
    expect(h).toHaveBeenCalledTimes(1);
  });

  it('passes an optional payload (project id, session id) to the handlers', () => {
    const style = vi.fn();
    const split = vi.fn();
    const fresh = vi.fn();
    const offs = [onUiCommand('openProjectStyle', style), onUiCommand('splitSession', split), onUiCommand('openNewSession', fresh)];
    emitUiCommand('openProjectStyle', 'p1');
    emitUiCommand('splitSession', 's7');
    emitUiCommand('openNewSession');
    emitUiCommand('openNewSession', 'p2');
    expect(style).toHaveBeenCalledWith('p1');
    expect(split).toHaveBeenCalledWith('s7');
    expect(fresh.mock.calls).toEqual([[undefined], ['p2']]);
    offs.forEach((off) => off());
  });
});
