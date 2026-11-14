import { describe, expect, it } from 'vitest';
import type { TranscriptTurn } from '../lib/client/types';
import { agentEdits } from './agentEdits';

const edit = (summary: string, sub_agent?: string, name = 'Edit'): TranscriptTurn => ({
  role: 'tool',
  text: '',
  tool: { name, summary },
  at: null,
  ...(sub_agent ? { sub_agent } : {}),
});

describe('agentEdits', () => {
  it('maps a changed file to the sub-agents whose Edit/Write turns name it', () => {
    const turns = [edit('src/a.ts', 'lint-fixer'), edit('src/a.ts', 'test-runner', 'Write'), edit('src/a.ts', 'lint-fixer'), edit('src/b.ts')];
    const map = agentEdits(turns, ['src/a.ts', 'src/b.ts', 'src/c.ts']);
    expect(map.get('src/a.ts')).toEqual(['lint-fixer', 'test-runner']);
    expect(map.has('src/b.ts')).toBe(false); // the main agent: no split
    expect(map.has('src/c.ts')).toBe(false);
  });

  it('matches an absolute summary by path suffix, never a partial name', () => {
    const map = agentEdits([edit('/work/repo/src/a.ts', 'x'), edit('/work/repo/src/data.ts', 'y')], ['src/a.ts', 'src/ta.ts', 'a.ts', 'src/data.ts']);
    expect(map.get('src/a.ts')).toEqual(['x']);
    expect(map.get('src/data.ts')).toEqual(['y']);
    expect(map.has('src/ta.ts')).toBe(false); // the tail of 'data.ts' without the separator
    expect(map.get('a.ts')).toEqual(['x']); // a bare name matches the file of that name
  });

  it('ignores tools that do not write files and turns without a tool', () => {
    const turns: TranscriptTurn[] = [edit('src/a.ts', 'x', 'Read'), edit('src/a.ts', 'x', 'Bash'), { role: 'assistant', text: 'hi', tool: null, at: null, sub_agent: 'x' }];
    expect(agentEdits(turns, ['src/a.ts']).size).toBe(0);
  });
});
