const isControl = (c: string): boolean => {
  const code = c.codePointAt(0) ?? 0;
  return code < 0x20 || code === 0x7f;
};

/** `$'…'` quoting: the only way to type a newline or other control character into a shell word. */
const ansiCQuote = (path: string): string =>
  "$'" +
  [...path]
    .map((c) => {
      if (c === '\\' || c === "'") return `\\${c}`;
      return isControl(c) ? `\\x${(c.codePointAt(0) ?? 0).toString(16).padStart(2, '0')}` : c;
    })
    .join('') +
  "'";

/** Escape a path for a shell prompt: backslashes like iTerm2 does for dropped files, `$'…'` if it has control characters. */
export const escapeDroppedPath = (path: string): string =>
  [...path].some(isControl) ? ansiCQuote(path) : path.replace(/[^\p{L}\p{N}_\-.,:/@%+=~]/gu, '\\$&');

/** Dropped files as shell words, with a trailing space so the next word can follow. */
export const droppedPathsText = (paths: string[]): string => paths.map(escapeDroppedPath).join(' ') + ' ';
