// Unified diff text → lines with a kind, for colouring. Before the first hunk the lines are the file's
// headers (`diff --git`, `index`, `---`, `+++`); inside a hunk the first character decides (a removed line
// that starts with "--" is still a removal, not a header).

export type DiffLineKind = 'meta' | 'hunk' | 'add' | 'del' | 'ctx';

export interface DiffLine {
  kind: DiffLineKind;
  text: string;
}

export function diffLines(diff: string): DiffLine[] {
  if (diff === '') return [];
  const lines = diff.replace(/\n$/, '').split('\n');
  let inHunk = false;
  return lines.map((text): DiffLine => {
    if (text.startsWith('@@')) {
      inHunk = true;
      return { kind: 'hunk', text };
    }
    if (!inHunk) return { kind: 'meta', text };
    if (text.startsWith('+')) return { kind: 'add', text };
    if (text.startsWith('-')) return { kind: 'del', text };
    if (text.startsWith('\\')) return { kind: 'meta', text }; // "\ No newline at end of file"
    return { kind: 'ctx', text };
  });
}

/** Added and removed line counts of one file's diff. */
export function diffCounts(diff: string): { added: number; removed: number } {
  let added = 0;
  let removed = 0;
  for (const l of diffLines(diff)) {
    if (l.kind === 'add') added++;
    else if (l.kind === 'del') removed++;
  }
  return { added, removed };
}
