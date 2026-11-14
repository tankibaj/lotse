import { act, render, screen } from '@testing-library/react';
import { afterEach, describe, expect, it, vi } from 'vitest';
import { closePanel, closeReview, getOpenPanel, getReview, isPanelName, openReview, subscribePanel, togglePanel, useOpenPanel } from './panel';

afterEach(() => {
  closePanel();
  closeReview();
});

describe('panel store', () => {
  it('toggles one panel at a time', () => {
    togglePanel('files');
    expect(getOpenPanel()).toBe('files');
    togglePanel('usage');
    expect(getOpenPanel()).toBe('usage');
    togglePanel('usage');
    expect(getOpenPanel()).toBeNull();
  });

  it('notifies once per change and re-renders hook users', () => {
    const l = vi.fn();
    const off = subscribePanel(l);
    togglePanel('session');
    closePanel();
    closePanel();
    expect(l).toHaveBeenCalledTimes(2);
    off();
    function Probe() {
      return <span>{useOpenPanel() ?? 'none'}</span>;
    }
    render(<Probe />);
    expect(screen.getByText('none')).toBeInTheDocument();
    act(() => togglePanel('changes'));
    expect(screen.getByText('changes')).toBeInTheDocument();
  });

  it('holds the review target (session and optional file) separately from the panel', () => {
    togglePanel('changes');
    openReview('a1', 'src/a.ts');
    expect(getReview()).toEqual({ sessionId: 'a1', file: 'src/a.ts' });
    expect(getOpenPanel()).toBe('changes');
    openReview('a2');
    expect(getReview()).toEqual({ sessionId: 'a2' });
    closeReview();
    expect(getReview()).toBeNull();
  });

  it('knows its panel names', () => {
    expect(['session', 'changes', 'files', 'transcript', 'usage'].every(isPanelName)).toBe(true);
    expect(isPanelName('diff')).toBe(false);
    expect(isPanelName(undefined)).toBe(false);
  });
});
