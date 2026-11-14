import { css } from '@codemirror/lang-css';
import { html } from '@codemirror/lang-html';
import { javascript } from '@codemirror/lang-javascript';
import { json } from '@codemirror/lang-json';
import { markdown } from '@codemirror/lang-markdown';
import { python } from '@codemirror/lang-python';
import { rust } from '@codemirror/lang-rust';
import type { Extension } from '@codemirror/state';

export interface Language {
  /** Status bar label. */
  name: string;
  /** `null`: plain text. */
  support: Extension | null;
}

const PLAIN: Language = { name: 'Plain text', support: null };

const BY_EXTENSION: Record<string, () => Language> = {
  ts: () => ({ name: 'TypeScript', support: javascript({ typescript: true }) }),
  mts: () => ({ name: 'TypeScript', support: javascript({ typescript: true }) }),
  cts: () => ({ name: 'TypeScript', support: javascript({ typescript: true }) }),
  tsx: () => ({ name: 'TypeScript', support: javascript({ typescript: true, jsx: true }) }),
  js: () => ({ name: 'JavaScript', support: javascript() }),
  mjs: () => ({ name: 'JavaScript', support: javascript() }),
  cjs: () => ({ name: 'JavaScript', support: javascript() }),
  jsx: () => ({ name: 'JavaScript', support: javascript({ jsx: true }) }),
  rs: () => ({ name: 'Rust', support: rust() }),
  py: () => ({ name: 'Python', support: python() }),
  json: () => ({ name: 'JSON', support: json() }),
  md: () => ({ name: 'Markdown', support: markdown() }),
  markdown: () => ({ name: 'Markdown', support: markdown() }),
  css: () => ({ name: 'CSS', support: css() }),
  html: () => ({ name: 'HTML', support: html() }),
  htm: () => ({ name: 'HTML', support: html() }),
};

/** Language by file extension; anything else is plain text. */
export function languageFor(path: string): Language {
  const file = path.slice(path.lastIndexOf('/') + 1);
  const dot = file.lastIndexOf('.');
  if (dot < 0) return PLAIN;
  return BY_EXTENSION[file.slice(dot + 1).toLowerCase()]?.() ?? PLAIN;
}

/** The file's line ending, from its first line break. */
export function detectEol(text: string): 'LF' | 'CRLF' {
  const i = text.indexOf('\n');
  return i > 0 && text[i - 1] === '\r' ? 'CRLF' : 'LF';
}

/** "Tabs", or "Spaces: N" with the smallest indent of 2+ spaces (1 space is a doc-comment continuation); 2 when nothing is indented. */
export function detectIndent(text: string): string {
  let tabs = 0;
  let spaces = 0;
  let smallest = Infinity;
  for (const line of text.split('\n')) {
    if (line.startsWith('\t')) tabs++;
    else if (line.startsWith('  ')) {
      spaces++;
      smallest = Math.min(smallest, /^ */.exec(line)![0].length);
    }
  }
  if (tabs > spaces) return 'Tabs';
  return `Spaces: ${Number.isFinite(smallest) ? smallest : 2}`;
}

/** 1-based line of the first difference between two texts (1 when they are equal): where the editor opens a changed file. */
export function firstChangedLine(base: string, current: string): number {
  const a = base.split('\n');
  const b = current.split('\n');
  const n = Math.min(a.length, b.length);
  for (let i = 0; i < n; i++) if (a[i] !== b[i]) return i + 1;
  return a.length === b.length ? 1 : n + 1;
}
