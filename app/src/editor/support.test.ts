import { describe, expect, it } from 'vitest';
import { detectEol, detectIndent, firstChangedLine, languageFor } from './support';

describe('languageFor', () => {
  it.each([
    ['src/a.ts', 'TypeScript'],
    ['src/a.tsx', 'TypeScript'],
    ['a.js', 'JavaScript'],
    ['a.mjs', 'JavaScript'],
    ['crates/x/src/lib.rs', 'Rust'],
    ['a.py', 'Python'],
    ['package.json', 'JSON'],
    ['README.md', 'Markdown'],
    ['a.css', 'CSS'],
    ['index.html', 'HTML'],
    ['Makefile', 'Plain text'],
    ['notes.unknown', 'Plain text'],
  ])('%s is %s', (path, name) => {
    const lang = languageFor(path);
    expect(lang.name).toBe(name);
    expect(lang.support === null).toBe(name === 'Plain text');
  });
});

describe('detectEol', () => {
  it('LF by default, CRLF when the first line break is CRLF', () => {
    expect(detectEol('')).toBe('LF');
    expect(detectEol('a\nb\n')).toBe('LF');
    expect(detectEol('a\r\nb\r\n')).toBe('CRLF');
    expect(detectEol('a\nb\r\n')).toBe('LF');
  });
});

describe('detectIndent', () => {
  it('tabs, the smallest space indent, or 2 spaces without indented lines', () => {
    expect(detectIndent('a\n\tb\n')).toBe('Tabs');
    expect(detectIndent('a\n    b\n        c\n')).toBe('Spaces: 4');
    expect(detectIndent('a\n  b\n')).toBe('Spaces: 2');
    expect(detectIndent('a\nb\n')).toBe('Spaces: 2');
  });
});

describe('firstChangedLine', () => {
  it('1-based line of the first difference, 1 for a new file', () => {
    expect(firstChangedLine('a\nb\nc\n', 'a\nB\nc\n')).toBe(2);
    expect(firstChangedLine('a\nb\n', 'a\nb\nc\n')).toBe(3);
    expect(firstChangedLine('', 'a\n')).toBe(1);
    expect(firstChangedLine('same\n', 'same\n')).toBe(1);
  });
});
