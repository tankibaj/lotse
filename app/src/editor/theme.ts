import { HighlightStyle, syntaxHighlighting } from '@codemirror/language';
import type { Extension } from '@codemirror/state';
import { EditorView } from '@codemirror/view';
import { tags as t } from '@lezer/highlight';
import { readXtermTheme } from '../terminal/xtermTheme';

const tint = (colour: string, pct: number) => `color-mix(in srgb, ${colour} ${pct}%, transparent)`;

/** Editor colours from the same tokens as the terminal (`readXtermTheme`), so it follows light, dark and auto; the font is the terminal's. */
export function editorTheme(fontFamily: string, fontSize: number, root: Element = document.documentElement): Extension {
  const c = readXtermTheme(root);
  const bg = c.background ?? '#ffffff';
  const fg = c.foreground ?? '#1d1d1f';
  const dim = c.brightBlack ?? '#8a8a8e';
  const sel = c.selectionBackground ?? 'rgba(0, 122, 255, 0.12)';
  const green = c.green ?? '#34c759';
  const red = c.red ?? '#ff3b30';
  const blue = c.blue ?? '#007aff';
  const dark = root.classList.contains('dark');
  const theme = EditorView.theme(
    {
      '&': { color: fg, backgroundColor: bg, height: '100%', fontSize: `${fontSize}px` },
      '&.cm-focused': { outline: 'none' },
      '.cm-scroller': { fontFamily, lineHeight: '1.6', overflow: 'auto' },
      '.cm-content': { caretColor: fg, padding: '8px 0' },
      '.cm-cursor, .cm-dropCursor': { borderLeftColor: fg },
      '&.cm-focused > .cm-scroller > .cm-selectionLayer .cm-selectionBackground, .cm-selectionBackground': { backgroundColor: sel },
      '.cm-activeLine': { backgroundColor: tint(blue, 5) },
      '.cm-gutters': { backgroundColor: bg, color: dim, border: 'none' },
      '.cm-activeLineGutter': { backgroundColor: 'transparent', color: fg },
      '.cm-changedLine': { backgroundColor: tint(green, 14) },
      '.cm-deletedChunk': { backgroundColor: tint(red, 12) },
      '.cm-deletedChunk .cm-deletedLine, .cm-deletedChunk del': { backgroundColor: 'transparent', textDecoration: 'none' },
      '.cm-changedText, .cm-insertedLine ins': { backgroundColor: tint(green, 28), textDecoration: 'none' },
      '.cm-deletedChunk .cm-changedText': { backgroundColor: tint(red, 26) },
      '.cm-changedLineGutter': { backgroundColor: green },
      '.cm-deletedLineGutter': { backgroundColor: red },
      '.cm-panels': { backgroundColor: bg, color: fg },
      '.cm-searchMatch': { backgroundColor: tint(c.yellow ?? '#ff9500', 30) },
    },
    { dark },
  );
  const highlight = HighlightStyle.define([
    { tag: [t.comment, t.docComment], color: dim, fontStyle: 'italic' },
    { tag: [t.keyword, t.controlKeyword, t.operatorKeyword, t.modifier], color: c.magenta },
    { tag: [t.string, t.special(t.string), t.regexp], color: c.yellow },
    { tag: [t.number, t.bool, t.null, t.atom], color: blue },
    { tag: [t.definition(t.variableName), t.function(t.variableName), t.function(t.propertyName)], color: c.cyan },
    { tag: [t.typeName, t.className, t.namespace], color: c.cyan },
    { tag: [t.heading], color: blue, fontWeight: '600' },
    { tag: t.strong, fontWeight: '600' },
    { tag: t.emphasis, fontStyle: 'italic' },
    { tag: [t.link, t.url], color: blue },
    { tag: t.invalid, color: red },
  ]);
  return [theme, syntaxHighlighting(highlight)];
}
