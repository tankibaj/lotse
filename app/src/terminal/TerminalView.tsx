import { useEffect, useRef, useState } from 'react';
import { FitAddon } from '@xterm/addon-fit';
import { WebLinksAddon } from '@xterm/addon-web-links';
import { WebglAddon } from '@xterm/addon-webgl';
import { Terminal } from '@xterm/xterm';
import '@xterm/xterm/css/xterm.css';
import { useClient } from '../lib/client';
import { hostBridge, inTauri } from '../sheets/hostBridge';
import type { SessionId } from '../lib/client';
import { DEFAULT_FONT_SIZE, FONT_WEIGHT, FONT_WEIGHT_BOLD, LINE_HEIGHT, terminalFontFamily } from './font';
import { droppedPathsText } from './dropPaths';
import { moveFocusOut } from './focus';
import { isLinkClick, openExternal } from './links';
import { registerTerminal } from './registry';
import { wantsWebgl } from './renderer';
import { readXtermTheme } from './xtermTheme';

export interface TerminalViewProps {
  sessionId: SessionId;
  /** Gets keyboard focus when true. */
  focused?: boolean;
  fontFamily?: string;
  fontSize?: number;
  /** Accessible name, e.g. "billing-api Claude Code". */
  label?: string;
  /** Change it to make a focused terminal take focus again. */
  focusSignal?: number;
  /** The pointer went down inside the terminal. */
  onFocus?: () => void;
}

/** Escape + carriage return: Alt+Enter, which prompts take as "insert a newline". */
const SHIFT_ENTER = '\x1b\r';
const SCROLLBACK_LINES = 10_000;
const encoder = new TextEncoder();
/** ⌘C / ⌘V / ⌘A / ⌘X stay with xterm (copy, paste, select all); every other ⌘ combo belongs to the app. */
const XTERM_META_KEYS = new Set(['c', 'v', 'a', 'x']);

/** Renderer: WebGL where the GPU allows it. Falls back to xterm's built-in DOM renderer (the canvas addon is gone in xterm 6). */
function loadWebgl(term: Terminal): WebglAddon | null {
  try {
    const webgl = new WebglAddon();
    webgl.onContextLoss(() => webgl.dispose());
    term.loadAddon(webgl);
    return webgl;
  } catch {
    return null;
  }
}

/**
 * One xterm.js terminal attached to a daemon session: replay on attach, keystrokes to the PTY, resize
 * to the daemon, ⌘-click links, light/dark theme. Sessions outlive the view (it detaches on unmount).
 */
export function TerminalView({
  sessionId,
  focused = true,
  fontFamily = terminalFontFamily(undefined),
  fontSize = DEFAULT_FONT_SIZE,
  label,
  focusSignal,
  onFocus,
}: TerminalViewProps) {
  const client = useClient();
  const host = useRef<HTMLDivElement>(null);
  const term = useRef<Terminal | null>(null);
  const fit = useRef<FitAddon | null>(null);
  const [failure, setFailure] = useState<string | null>(null);
  // The font the terminal is created with; later changes go through the effect below.
  const initialFont = useRef({ fontFamily, fontSize });
  useEffect(() => {
    initialFont.current = { fontFamily, fontSize };
  });

  useEffect(() => {
    const el = host.current;
    if (!el) return;
    let t: Terminal;
    const fitAddon = new FitAddon();
    try {
      t = new Terminal({
        allowProposedApi: false,
        cursorBlink: true,
        fontFamily: initialFont.current.fontFamily,
        fontSize: initialFont.current.fontSize,
        lineHeight: LINE_HEIGHT,
        fontWeight: FONT_WEIGHT,
        fontWeightBold: FONT_WEIGHT_BOLD,
        // OSC 8 hyperlinks: xterm's default handler uses confirm() + window.open(), a no-op in WKWebView.
        linkHandler: {
          activate: (e, uri) => {
            if (isLinkClick(e)) openExternal(uri);
          },
        },
        scrollback: SCROLLBACK_LINES,
        theme: readXtermTheme(),
      });
      t.loadAddon(fitAddon);
      t.loadAddon(new WebLinksAddon((e, uri) => isLinkClick(e) && openExternal(uri)));
      t.open(el);
    } catch (err) {
      // No usable DOM/canvas (very old WebView, test runner): say so instead of a blank pane.
      const message = err instanceof Error ? err.message : String(err);
      queueMicrotask(() => setFailure(message));
      return;
    }
    if (wantsWebgl()) loadWebgl(t);
    t.attachCustomKeyEventHandler((e) => {
      // Ctrl+Tab / Ctrl+Shift+Tab: the keyboard way out of the terminal (Tab itself belongs to the shell).
      if (e.key === 'Tab' && e.ctrlKey && !e.metaKey && !e.altKey) {
        if (e.type === 'keydown' && t.textarea) {
          e.preventDefault();
          moveFocusOut(t.textarea, e.shiftKey ? -1 : 1);
        }
        return false;
      }
      // Shift+Enter: a new line in the prompt, not a submit (xterm sends a plain \r). ESC + CR is what Claude Code
      // and most TUIs read as "newline", like iTerm2 with Shift+Enter mapped to it.
      if (e.key === 'Enter' && e.shiftKey && !e.ctrlKey && !e.metaKey && !e.altKey) {
        if (e.type === 'keydown') {
          e.preventDefault();
          t.input(SHIFT_ENTER);
        }
        return false;
      }
      return !(e.metaKey && !XTERM_META_KEYS.has(e.key.toLowerCase()));
    });
    term.current = t;
    fit.current = fitAddon;
    const unregister = registerTerminal(sessionId, t);
    const refit = () => {
      if (el.clientWidth > 0 && el.clientHeight > 0) fitAddon.fit();
    };
    refit();

    const theme = new MutationObserver(() => {
      t.options.theme = readXtermTheme();
    });
    theme.observe(document.documentElement, { attributes: true, attributeFilter: ['class'] });
    let frame = 0;
    const resizeObserver =
      typeof ResizeObserver === 'undefined'
        ? null
        : new ResizeObserver(() => {
            cancelAnimationFrame(frame);
            frame = requestAnimationFrame(refit);
          });
    resizeObserver?.observe(el);

    // Attach one tick later: React StrictMode mounts, unmounts and remounts at once, and a replay
    // must not be requested for the mount that is already gone.
    let disposed = false;
    let requested = false; // an attach was sent, so cleanup detaches even if its answer is still pending
    let live = false; // attached: input and resizes go to the daemon
    const offData = client.onPtyData(sessionId, (data) => t.write(data));
    const attach = () => {
      requested = true;
      return client.attach(sessionId, t.cols, t.rows).then(
        () => {
          live = !disposed;
        },
        (err: unknown) => {
          live = false;
          if (!disposed) t.write(`\r\n\x1b[31m[lotse] attach failed: ${err instanceof Error ? err.message : String(err)}\x1b[0m\r\n`);
        },
      );
    };
    // Input handlers live as long as the terminal; `live` gates them, so they also work after a re-attach.
    const disposables = [
      t.onData((data) => live && client.writePty(sessionId, encoder.encode(data))),
      t.onBinary((data) => live && client.writePty(sessionId, Uint8Array.from(data, (c) => c.charCodeAt(0)))),
      t.onResize(({ cols, rows }) => live && void client.resize(sessionId, cols, rows).catch(() => {})),
    ];
    // Files dropped from Finder: type their escaped paths at the prompt (iTerm2 does the same).
    // Only in the app: a browser exposes no real paths (its bridge returns placeholders).
    const offDrops = inTauri()
      ? hostBridge().watchDrops(() => el, {
          onHover: () => {},
          onDrop: (paths) => {
            if (!paths.length) return;
            t.paste(droppedPathsText(paths));
            t.focus();
          },
        })
      : () => {};
    const start = window.setTimeout(() => void attach(), 0);
    // The link came back after a drop: output was missed, so start from the daemon's replay again.
    const offConn = client.onConnection((state) => {
      if (state === 'reconnected' && !disposed) {
        live = false;
        t.reset();
        void attach();
      }
    });

    return () => {
      disposed = true;
      window.clearTimeout(start);
      cancelAnimationFrame(frame);
      resizeObserver?.disconnect();
      theme.disconnect();
      offConn();
      offDrops();
      offData();
      disposables.forEach((d) => d.dispose());
      if (requested) void client.detach(sessionId).catch(() => {});
      unregister();
      term.current = null;
      fit.current = null;
      t.dispose();
    };
  }, [client, sessionId]);

  // Font settings and ⌘+ / ⌘−: change the live terminal, then refit so cols and rows follow.
  useEffect(() => {
    const t = term.current;
    if (!t) return;
    t.options.fontFamily = fontFamily;
    t.options.fontSize = fontSize;
    fit.current?.fit();
  }, [fontFamily, fontSize, sessionId]);

  // Take keyboard focus when this is the focused pane, except from a tab the user is arrowing through.
  // `focusSignal` asks again (e.g. the control that had focus was removed).
  useEffect(() => {
    if (focused && document.activeElement?.getAttribute('role') !== 'tab') term.current?.focus();
  }, [focused, sessionId, focusSignal]);

  return (
    <div
      className="relative min-h-0 min-w-0 flex-1 overflow-hidden bg-term px-5 py-4"
      role="group"
      aria-label={label ? `Terminal ${label}` : 'Terminal'}
      aria-description="Control+Tab moves focus out of the terminal"
      data-testid="terminal"
      data-session-terminal={sessionId}
      onMouseDown={onFocus}
    >
      <div ref={host} className="h-full w-full" />
      {failure && (
        <p className="absolute inset-0 grid place-items-center p-6 text-center text-tdim" role="alert">
          Terminal unavailable: {failure}
        </p>
      )}
    </div>
  );
}
