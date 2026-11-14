import { act, render, screen } from '@testing-library/react';
import { afterEach, describe, expect, it, vi } from 'vitest';
import { getSelectedSessionId, selectSession, subscribeSelection, useSelectedSessionId } from './selection';

afterEach(() => selectSession(null));

describe('selection', () => {
  it('notifies once per change and not for the same id', () => {
    const l = vi.fn();
    const off = subscribeSelection(l);
    selectSession('a1');
    selectSession('a1');
    selectSession('a3');
    expect(l).toHaveBeenCalledTimes(2);
    expect(getSelectedSessionId()).toBe('a3');
    off();
    selectSession('a4');
    expect(l).toHaveBeenCalledTimes(2);
  });

  it('re-renders hook users', () => {
    function Probe() {
      return <span>{useSelectedSessionId() ?? 'none'}</span>;
    }
    render(<Probe />);
    expect(screen.getByText('none')).toBeInTheDocument();
    act(() => selectSession('a2'));
    expect(screen.getByText('a2')).toBeInTheDocument();
  });
});
