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
});
