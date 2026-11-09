import { afterEach, describe, expect, it } from 'vitest';
import { buildReviewPrompt, clearComments, dropEmptyComments, getComments, putComment, removeComment } from './comments';

afterEach(() => clearComments('s1'));

describe('buildReviewPrompt', () => {
  it('groups by file, orders by line, quotes the line and skips empty comments', () => {
    const prompt = buildReviewPrompt([
      { path: 'b.ts', line: 9, quote: '  const x = 1', text: 'rename' },
      { path: 'a.ts', line: 30, quote: 'late', text: 'second' },
      { path: 'a.ts', line: 3, quote: 'early', text: '  first\nover two lines  ' },
      { path: 'a.ts', line: 4, quote: 'blank', text: '   ' },
    ]);
    // files in the order their first comment came, lines ascending within a file
    expect(prompt).toBe(
      [
        'Review comments on your changes. Please address each one:',
        'b.ts:9\n> const x = 1\nrename',
        'a.ts:3\n> early\nfirst\nover two lines',
        'a.ts:30\n> late\nsecond',
      ].join('\n\n'),
    );
  });

  it('cuts a very long quote', () => {
    const prompt = buildReviewPrompt([{ path: 'a.ts', line: 1, quote: 'x'.repeat(500), text: 'long' }]);
    expect(prompt).toContain(`> ${'x'.repeat(200)}…\n`);
    expect(prompt).not.toContain('x'.repeat(201));
  });
});

describe('comment store', () => {
  it('replaces the comment on the same line, removes, and drops empty boxes', () => {
    putComment('s1', { path: 'a.ts', line: 1, quote: 'q', text: '' });
    putComment('s1', { path: 'a.ts', line: 1, quote: 'q', text: 'one' });
    putComment('s1', { path: 'a.ts', line: 2, quote: 'q', text: '' });
    expect(getComments('s1').map((c) => `${c.line}:${c.text}`)).toEqual(['1:one', '2:']);
    dropEmptyComments('s1');
    expect(getComments('s1').map((c) => c.line)).toEqual([1]);
    removeComment('s1', 'a.ts', 1);
    expect(getComments('s1')).toEqual([]);
  });

  it('keeps sessions apart', () => {
    putComment('s1', { path: 'a.ts', line: 1, quote: 'q', text: 'x' });
    expect(getComments('s2')).toEqual([]);
  });
});
