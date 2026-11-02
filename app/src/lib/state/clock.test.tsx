import { act, renderHook } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { useCoarseNow } from './clock';

// The sidebar must not re-render per second: only when a visible label changes, never while held.

describe('useCoarseNow', () => {
  beforeEach(() => {
    vi.useFakeTimers();
    vi.setSystemTime(0);
  });
  afterEach(() => vi.useRealTimers());

  const minute = (now: number) => String(Math.floor(now / 60_000));

  it('does not re-render until the visible text would change', () => {
    let renders = 0;
    const hold = () => false;
    renderHook(() => {
      renders++;
      return useCoarseNow(minute, hold);
    });
    const first = renders;
    act(() => void vi.advanceTimersByTime(59_000));
    expect(renders).toBe(first);
    act(() => void vi.advanceTimersByTime(2_000));
    expect(renders).toBe(first + 1);
  });

  it('defers while held and catches up on the first tick after release', () => {
    let held = true;
    const hold = () => held;
    const { result } = renderHook(() => useCoarseNow(minute, hold));
    act(() => void vi.advanceTimersByTime(5 * 60_000));
    expect(result.current).toBe(0);
    held = false;
    act(() => void vi.advanceTimersByTime(1_000));
    expect(result.current).toBeGreaterThanOrEqual(5 * 60_000);
  });
});
