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

  it('passes a second payload (review file, editor path) only when given', () => {
    const review = vi.fn();
    const file = vi.fn();
    const offs = [onUiCommand('openReview', review), onUiCommand('openFile', file)];
    emitUiCommand('openReview', 'a1');
    emitUiCommand('openReview', 'a1', 'src/a.ts');
    emitUiCommand('openFile', 'a1', 'README.md');
    expect(review.mock.calls).toEqual([['a1'], ['a1', 'src/a.ts']]);
    expect(file).toHaveBeenCalledWith('a1', 'README.md');
    offs.forEach((off) => off());
  });

  it('has the panel and composer commands', () => {
    const h = vi.fn();
    const offs = [onUiCommand('togglePanel', h), onUiCommand('openComposer', h)];
    expect(emitUiCommand('togglePanel', 'files')).toBe(1);
    expect(emitUiCommand('openComposer')).toBe(1);
    expect(h.mock.calls).toEqual([['files'], [undefined]]);
    offs.forEach((off) => off());
  });
});
