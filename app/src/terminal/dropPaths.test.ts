import { describe, expect, it } from 'vitest';
import { droppedPathsText, escapeDroppedPath } from './dropPaths';

describe('dropped paths', () => {
  it('escapes spaces and shell metacharacters', () => {
    expect(escapeDroppedPath('/Users/n/Screenshot 2026 (1).png')).toBe('/Users/n/Screenshot\\ 2026\\ \\(1\\).png');
    expect(escapeDroppedPath("/tmp/it's$x;y.png")).toBe("/tmp/it\\'s\\$x\\;y.png");
  });
  it('leaves plain paths alone', () => {
    expect(escapeDroppedPath('/tmp/a-b_c.d/e.png')).toBe('/tmp/a-b_c.d/e.png');
  });
  it('keeps non-ASCII letters and escapes whole astral characters', () => {
    expect(escapeDroppedPath('/tmp/Übersicht.png')).toBe('/tmp/Übersicht.png');
    expect(escapeDroppedPath('/tmp/📄 a.txt')).toBe('/tmp/\\📄\\ a.txt');
  });
  it('quotes control characters with $\'…\'', () => {
    expect(escapeDroppedPath('/tmp/a\nb\'c')).toBe("$'/tmp/a\\x0ab\\'c'");
  });
  it('joins several files with spaces and ends with one', () => {
    expect(droppedPathsText(['/a b.png', '/c.png'])).toBe('/a\\ b.png /c.png ');
  });
});
