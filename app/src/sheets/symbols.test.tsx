import { render, waitFor } from '@testing-library/react';
import { afterEach, describe, expect, it, vi } from 'vitest';
import { LUCIDE } from './lucideIcons';
import { setSymbolBackend } from './nativeSymbols';
import { ProjectIcon, SYMBOLS, symbolEntry } from './symbols';

afterEach(() => setSymbolBackend(undefined));

// The ids the Lucide-only picker saved before SF Symbols: every one must keep resolving.
const LEGACY = [
  'folder', 'terminal', 'code', 'braces', 'git-branch', 'bug', 'hammer', 'wrench', 'cpu', 'database', 'server', 'cloud',
  'package', 'flask-conical', 'rocket', 'shield', 'book', 'briefcase', 'house', 'building-2', 'shopping-cart', 'globe',
  'camera', 'music', 'heart-pulse', 'stethoscope', 'graduation-cap', 'gamepad-2', 'star', 'heart', 'anchor', 'ship',
  'flame', 'leaf', 'flag', 'sparkles', 'sun', 'moon', 'zap', 'lock', 'key',
];

describe('symbol catalogue', () => {
  it('has about 150 symbols with unique ids, an SF Symbol and a Lucide stand-in each', () => {
    expect(SYMBOLS.length).toBeGreaterThanOrEqual(140);
    expect(SYMBOLS.length).toBeLessThanOrEqual(180);
    expect(new Set(SYMBOLS.map((s) => s.id)).size).toBe(SYMBOLS.length);
    for (const s of SYMBOLS) expect(s.sf, s.id).toMatch(/^[a-z0-9]+(\.[a-z0-9]+)*$/);
    expect(SYMBOLS.filter((s) => !LUCIDE[s.lucide]).map((s) => `${s.id}: ${s.lucide}`)).toEqual([]);
  });

  it('keeps every id the Lucide-only picker saved', () => {
    for (const id of LEGACY) expect(symbolEntry(id), id).toBeDefined();
  });

  it('finds a symbol by SF Symbol name too', () => {
    expect(symbolEntry('ladybug.fill')?.id).toBe('bug');
  });
});

describe('ProjectIcon without a native host (browser, Mock)', () => {
  it('draws the Lucide stand-in', () => {
    const { container } = render(<ProjectIcon name="rocket" />);
    expect(container.querySelector('svg')).toBeInTheDocument();
    expect(container.querySelector('[data-symbol]')).toBeNull();
  });

  it('draws the folder for a name nobody knows', () => {
    const { container } = render(<ProjectIcon name="no-such-icon" />);
    expect(container.querySelector('svg')).toBeInTheDocument();
  });
});

describe('ProjectIcon with a native host', () => {
  it('asks for the SF Symbol and draws it as a mask in the text colour', async () => {
    const png = vi.fn(async () => 'iVBORw0KGgo=');
    setSymbolBackend({ png });
    const { container } = render(<ProjectIcon name="git-branch" size={16} />);
    const glyph = await waitFor(() => {
      const el = container.querySelector('[data-symbol]');
      expect(el).toBeInTheDocument();
      return el as HTMLElement;
    });
    expect(glyph).toHaveAttribute('data-symbol', 'arrow.triangle.branch');
    expect(glyph).toHaveClass('bg-current');
    expect(png).toHaveBeenCalledWith('arrow.triangle.branch', 16, 1);
    expect(container.querySelector('svg')).toBeNull();
  });

  it('asks once per symbol and size, however many icons show it', async () => {
    const png = vi.fn(async () => 'AAAA');
    setSymbolBackend({ png });
    const { container } = render(
      <>
        <ProjectIcon name="folder" size={14} />
        <ProjectIcon name="folder" size={14} />
        <ProjectIcon name="folder" size={20} />
      </>,
    );
    await waitFor(() => expect(container.querySelectorAll('[data-symbol]')).toHaveLength(3));
    expect(png).toHaveBeenCalledTimes(2);
  });

  it('falls back to Lucide when the Mac has no such symbol', async () => {
    const png = vi.fn(async () => {
      throw new Error('unknown SF Symbol: ladybug.fill');
    });
    setSymbolBackend({ png });
    const { container } = render(<ProjectIcon name="bug" />);
    await waitFor(() => expect(container.querySelector('svg')).toBeInTheDocument());
    expect(png).toHaveBeenCalledTimes(1);
    expect(container.querySelector('[data-symbol]')).toBeNull();
  });

  it('asks for a saved SF Symbol name as it is; names that cannot be symbols never reach the host', async () => {
    const png = vi.fn(async () => 'AAAA');
    setSymbolBackend({ png });
    const { container } = render(
      <>
        <ProjectIcon name="hammer.circle.fill" />
        <ProjectIcon name="No Such Icon!" />
      </>,
    );
    await waitFor(() => expect(container.querySelector('[data-symbol="hammer.circle.fill"]')).toBeInTheDocument());
    expect(png).toHaveBeenCalledTimes(1);
    expect(container.querySelector('svg')).toBeInTheDocument(); // the folder, for the second one
  });

  it('keeps the icon\'s space while the host answers', async () => {
    let answer: (png: string) => void = () => {};
    setSymbolBackend({ png: () => new Promise<string>((r) => (answer = r)) });
    const { container } = render(<ProjectIcon name="star" size={18} />);
    const slot = container.querySelector('[data-symbol-pending]') as HTMLElement;
    expect(slot).toHaveStyle({ width: '18px', height: '18px' });
    expect(container.querySelector('svg')).toBeNull();
    answer('AAAA');
    await waitFor(() => expect(container.querySelector('[data-symbol="star.fill"]')).toBeInTheDocument());
  });
});
