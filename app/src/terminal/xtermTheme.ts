import type { ITheme } from '@xterm/xterm';

/** Theme tokens (theme/tokens.css) mapped onto xterm colours, so the terminal follows light, dark and auto like the mock's `.term`. */
export function readXtermTheme(root: Element = document.documentElement): ITheme {
  const style = getComputedStyle(root);
  const v = (name: string, fallback: string) => style.getPropertyValue(name).trim() || fallback;
  const txt = v('--txt', '#1d1d1f');
  const dim = v('--tdim', '#8a8a8e');
  const bg = v('--term', '#ffffff');
  const red = v('--red', '#ff3b30');
  const green = v('--green', '#34c759');
  const orange = v('--orange', '#ff9500');
  const blue = v('--blue', '#007aff');
  const purple = v('--c-purple', '#af52de');
  const cyan = v('--c-cyan', '#32ade6');
  return {
    background: bg,
    foreground: txt,
    cursor: txt,
    cursorAccent: bg,
    selectionBackground: v('--sel', 'rgba(0, 122, 255, 0.12)'),
    // Quiet overlay scrollbar (xterm's default is a heavy grey bar that the mock does not have).
    scrollbarSliderBackground: 'rgba(128, 128, 128, 0.18)',
    scrollbarSliderHoverBackground: 'rgba(128, 128, 128, 0.32)',
    scrollbarSliderActiveBackground: 'rgba(128, 128, 128, 0.45)',
    black: txt,
    red,
    green,
    yellow: orange,
    blue,
    magenta: purple,
    cyan,
    white: v('--dim', '#6e6e73'),
    brightBlack: dim,
    brightRed: red,
    brightGreen: green,
    brightYellow: orange,
    brightBlue: blue,
    brightMagenta: purple,
    brightCyan: cyan,
    brightWhite: txt,
  };
}
