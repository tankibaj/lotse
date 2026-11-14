import { readFileSync } from 'node:fs';
import { describe, expect, it } from 'vitest';

// vitest runs from app/
const css = readFileSync('src/theme/tokens.css', 'utf8');
import { THEME_KEY, applyTheme, getThemePref, resolveDark, setTheme } from './theme';

describe('theme', () => {
  it('resolves light, dark and auto', () => {
    expect(resolveDark('light', true)).toBe(false);
    expect(resolveDark('dark', false)).toBe(true);
    expect(resolveDark('auto', true)).toBe(true);
    expect(resolveDark('auto', false)).toBe(false);
  });

  it('defaults to auto and ignores junk in storage', () => {
    expect(getThemePref()).toBe('auto');
    localStorage.setItem(THEME_KEY, 'sepia');
    expect(getThemePref()).toBe('auto');
  });

  it('saves the choice and toggles the dark class on <html>', () => {
    setTheme('dark');
    expect(localStorage.getItem(THEME_KEY)).toBe('dark');
    expect(document.documentElement).toHaveClass('dark');
    applyTheme('light');
    expect(document.documentElement).not.toHaveClass('dark');
  });

  it('keeps the mock token values (light and dark)', () => {
    const block = (sel: string) => css.slice(css.indexOf(sel), css.indexOf('}', css.indexOf(sel)));
    const light = block(':root {');
    const dark = block('.dark {');
    for (const v of ['--win: #ffffff', '--side: #f5f5f7', '--line: #e5e5ea', '--txt: #1d1d1f', '--blue: #007aff', '--term: #ffffff']) {
      expect(light).toContain(v);
    }
    for (const v of ['--win: #1e1e1e', '--side: #252527', '--line: #38383a', '--txt: #f5f5f7', '--blue: #0a84ff', '--term: #1e1e1e']) {
      expect(dark).toContain(v);
    }
  });
});
