import { afterEach, describe, expect, it, vi } from 'vitest';
import { MAX_FONT_SIZE, MIN_FONT_SIZE, clampFontSize, terminalFontFamily } from './font';
import { isLinkClick, openExternal } from './links';
import { RENDERER_KEY, wantsWebgl } from './renderer';
import { readXtermTheme } from './xtermTheme';

describe('terminalFontFamily', () => {
  it('puts the chosen family first, quoted, then the mono stack', () => {
    expect(terminalFontFamily('SF Mono')).toBe('"SF Mono", "SF Mono", Menlo, Monaco, monospace');
  });

  it('uses the stack alone for empty and system names', () => {
    for (const name of [undefined, '', '  ', 'System (SF Mono)']) {
      expect(terminalFontFamily(name)).toBe('"SF Mono", Menlo, Monaco, monospace');
    }
  });

  it('cannot break out of the quotes', () => {
    expect(terminalFontFamily('Evil"; color: red')).toBe('"Evil; color: red", "SF Mono", Menlo, Monaco, monospace');
  });
});

describe('clampFontSize', () => {
  it('keeps sizes in range and whole', () => {
    expect(clampFontSize(2)).toBe(MIN_FONT_SIZE);
    expect(clampFontSize(99)).toBe(MAX_FONT_SIZE);
    expect(clampFontSize(13.4)).toBe(13);
    expect(clampFontSize(Number.NaN)).toBe(13);
  });
});

describe('links', () => {
  afterEach(() => vi.restoreAllMocks());

  it('opens http and https in a new window without opener access', () => {
    const open = vi.spyOn(window, 'open').mockReturnValue(null);
    expect(openExternal('https://example.com/a?b=1')).toBe(true);
    expect(open).toHaveBeenCalledWith('https://example.com/a?b=1', '_blank', 'noopener,noreferrer');
  });

  it('refuses every other scheme and garbage', () => {
    const open = vi.spyOn(window, 'open').mockReturnValue(null);
    for (const uri of ['javascript:alert(1)', 'file:///etc/passwd', 'ssh://host', 'not a url', '']) {
      expect(openExternal(uri), uri).toBe(false);
    }
    expect(open).not.toHaveBeenCalled();
  });

  it('a link opens on ⌘-click or ⌃-click only', () => {
    expect(isLinkClick({ metaKey: true, ctrlKey: false })).toBe(true);
    expect(isLinkClick({ metaKey: false, ctrlKey: true })).toBe(true);
    expect(isLinkClick({ metaKey: false, ctrlKey: false })).toBe(false);
  });
});

describe('wantsWebgl', () => {
  it('is off (DOM renderer) unless the user opted in', () => {
    expect(wantsWebgl()).toBe(false);
    localStorage.setItem(RENDERER_KEY, 'dom');
    expect(wantsWebgl()).toBe(false);
    localStorage.setItem(RENDERER_KEY, 'webgl');
    expect(wantsWebgl()).toBe(true);
  });
});

describe('readXtermTheme', () => {
  afterEach(() => document.documentElement.removeAttribute('style'));

  it('follows the theme tokens, so light and dark change the terminal colours', () => {
    document.documentElement.style.setProperty('--term', '#1e1e1e');
    document.documentElement.style.setProperty('--txt', '#f5f5f7');
    document.documentElement.style.setProperty('--orange', '#ff9f0a');
    const theme = readXtermTheme();
    expect(theme.background).toBe('#1e1e1e');
    expect(theme.foreground).toBe('#f5f5f7');
    expect(theme.yellow).toBe('#ff9f0a');
  });

  it('falls back to the light palette without tokens', () => {
    expect(readXtermTheme().background).toBe('#ffffff');
  });
});
