import { defaultKeymap, history, historyKeymap, indentWithTab } from '@codemirror/commands';
import { bracketMatching, indentOnInput, indentUnit } from '@codemirror/language';
import { highlightSelectionMatches, search, searchKeymap } from '@codemirror/search';
import type { Text } from '@codemirror/state';
import { Compartment, EditorState } from '@codemirror/state';
import { drawSelection, EditorView, highlightActiveLine, highlightActiveLineGutter, keymap, lineNumbers } from '@codemirror/view';
import { unifiedMergeView } from '@codemirror/merge';
import { GitCompare } from 'lucide-react';
import { useEffect, useRef, useState } from 'react';
import { DaemonError, useClient } from '../lib/client';
import type { SessionId } from '../lib/client';
import { useTerminalAppearance } from '../terminal/useTerminalAppearance';
import { detectEol, detectIndent, firstChangedLine, languageFor } from './support';
import { editorTheme } from './theme';

/** What the agent changed in this file: the change bar and the Diff toggle show only when set. */
export interface EditorChange {
  added: number;
  removed: number;
  /** "claude in “Fix totals”" */
  by: string;
}

export interface EditorProps {
  sessionId: SessionId;
  /** Relative to the session folder. */
  path: string;
  change?: EditorChange;
  /** What the Diff toggle compares against. Default `HEAD`. */
  baseRev?: string;
  /** "Review ⇧⌘R": the host opens the full review. */
  onReview?: () => void;
  /** Unsaved edits exist (edited, saving, or a save that failed). Pass a stable function. */
  onDirtyChange?: (dirty: boolean) => void;
}

type Save = 'saved' | 'edited' | 'saving' | 'conflict' | 'error';
type Load =
  | { state: 'loading' }
  | { state: 'error'; message: string }
  | { state: 'ready'; binary: boolean; size: number; indent: string; eol: string };

const themeC = new Compartment();
const diffC = new Compartment();
const lockedExt = [EditorState.readOnly.of(true), EditorView.editable.of(false)];

const errorText = (e: unknown) => (e instanceof Error ? e.message : String(e));

/** The file as the agent's base saw it: a file missing there is new, so everything counts as added. `null`: unknown. */
const readBase = (client: ReturnType<typeof useClient>, sessionId: SessionId, path: string, baseRev: string) =>
  client.readFile(sessionId, path, baseRev).then(
    (f) => f.text,
    (e: unknown) => (e instanceof DaemonError && e.code === 'not_found' ? '' : null),
  );

/** One file in CodeMirror 6, edited through the daemon: ⌘S writes it guarded by the sha it was read with. */
export function Editor(props: EditorProps) {
  // A new file is a new editor: nothing carries over.
  return <EditorBody key={`${props.sessionId}:${props.path}`} {...props} />;
}

function EditorBody({ sessionId, path, change, baseRev = 'HEAD', onReview, onDirtyChange }: EditorProps) {
  const client = useClient();
  const { fontFamily, fontSize } = useTerminalAppearance();
  const hostRef = useRef<HTMLDivElement>(null);
  const viewRef = useRef<EditorView | null>(null);
  const shaRef = useRef('');
  const savedDoc = useRef<Text | null>(null);
  const reviewRef = useRef(onReview);
  const [load, setLoad] = useState<Load>({ state: 'loading' });
  const [save, setSave] = useState<Save>('saved');
  const [saveError, setSaveError] = useState('');
  const [fetchedBase, setFetchedBase] = useState<string | null>(null);
  const [diff, setDiff] = useState(false);
  const [reloads, setReloads] = useState(0);
  const [themeTick, setThemeTick] = useState(0);
  const hasChange = change !== undefined;
  const base = hasChange ? fetchedBase : null;
  const changeSig = change ? `${change.added}:${change.removed}:${change.by}` : '';
  const ready = load.state === 'ready';
  // The agent's change as of the latest render, and as of the last time the diff base was read. The load below reads
  // the first, but does not depend on it: a change arriving later must not destroy the view (and the user's unsaved
  // buffer); the effect after the load deals with it.
  const latest = useRef({ hasChange, baseRev });
  const asked = useRef({ hasChange, baseRev });
  const [seenSig, setSeenSig] = useState(changeSig);
  const [external, setExternal] = useState(false);

  useEffect(() => {
    latest.current = { hasChange, baseRev };
  }, [hasChange, baseRev]);

  useEffect(() => {
    reviewRef.current = onReview;
  }, [onReview]);

  /** Drop the buffer and read the file again. */
  const reload = () => {
    setLoad({ state: 'loading' });
    setSave('saved');
    setExternal(false);
    setDiff(false);
    setReloads((n) => n + 1);
  };

  // The agent changed this file (the host passes a new `change`). A clean buffer just follows the disk; unsaved edits
  // are kept and the user chooses (Reload / Keep mine), instead of being thrown away with the view. State adjusted
  // while rendering, as React documents for a state that follows a prop.
  if (seenSig !== changeSig) {
    setSeenSig(changeSig);
    if (changeSig !== '') {
      if (!ready || save === 'saved') reload();
      else setExternal(true);
    }
  }
  // Edits undone while the notice was up: nothing left to protect, take the disk version.
  if (external && save === 'saved') reload();

  useEffect(() => {
    onDirtyChange?.(save !== 'saved');
  }, [save, onDirtyChange]);

  // Light/dark/auto is the `dark` class on <html> (theme/theme.ts).
  useEffect(() => {
    const obs = new MutationObserver(() => setThemeTick((n) => n + 1));
    obs.observe(document.documentElement, { attributes: true, attributeFilter: ['class'] });
    return () => obs.disconnect();
  }, []);

  useEffect(() => {
    let alive = true;
    let view: EditorView | null = null;
    const { hasChange, baseRev } = latest.current;
    asked.current = { hasChange, baseRev };
    const baseText = hasChange ? readBase(client, sessionId, path, baseRev) : Promise.resolve(null);

    const doSave = () => {
      if (!view) return;
      const doc = view.state.doc;
      if (savedDoc.current?.eq(doc)) return;
      setSave('saving');
      client.writeFile(sessionId, path, view.state.sliceDoc(), shaRef.current).then(
        (f) => {
          shaRef.current = f.sha;
          savedDoc.current = doc;
          setSave(view && !view.state.doc.eq(doc) ? 'edited' : 'saved');
        },
        (e: unknown) => {
          setSaveError(errorText(e));
          setSave(e instanceof DaemonError && e.code === 'conflict' ? 'conflict' : 'error');
        },
      );
    };

    Promise.all([client.readFile(sessionId, path), baseText]).then(
      ([file, baseDoc]) => {
        if (!alive || !hostRef.current) return;
        const indent = detectIndent(file.text);
        const eol = detectEol(file.text);
        const lang = languageFor(path);
        const doc = file.binary ? '' : file.text;
        const state = EditorState.create({
          doc,
          extensions: [
            lineNumbers(),
            highlightActiveLine(),
            highlightActiveLineGutter(),
            drawSelection(),
            history(),
            bracketMatching(),
            indentOnInput(),
            highlightSelectionMatches(),
            search({ top: true }),
            indentUnit.of(indent === 'Tabs' ? '\t' : ' '.repeat(Number(indent.slice('Spaces: '.length)))),
            EditorState.lineSeparator.of(eol === 'CRLF' ? '\r\n' : '\n'),
            EditorView.contentAttributes.of({ 'aria-label': `Editor ${path}` }),
            lang.support ?? [],
            themeC.of([]),
            diffC.of([]),
            file.binary
              ? lockedExt
              : [
                  EditorView.updateListener.of((u) => {
                    if (!u.docChanged) return;
                    setSave((s) => (s === 'saving' || s === 'conflict' ? s : savedDoc.current?.eq(u.state.doc) ? 'saved' : 'edited'));
                  }),
                  keymap.of([
                    { key: 'Meta-s', preventDefault: true, run: () => (doSave(), true) },
                    { key: 'Shift-Meta-r', preventDefault: true, run: () => (reviewRef.current?.(), true) },
                    ...searchKeymap,
                    ...historyKeymap,
                    ...defaultKeymap,
                    indentWithTab,
                  ]),
                ],
          ],
        });
        view = new EditorView({ state, parent: hostRef.current });
        viewRef.current = view;
        shaRef.current = file.sha;
        savedDoc.current = view.state.doc;
        if (baseDoc !== null) {
          const line = view.state.doc.line(Math.min(firstChangedLine(baseDoc, doc), view.state.doc.lines));
          view.dispatch({ selection: { anchor: line.from }, effects: EditorView.scrollIntoView(line.from, { y: 'center' }) });
        }
        setFetchedBase(baseDoc);
        setLoad({ state: 'ready', binary: file.binary, size: file.size, indent, eol });
      },
      (e: unknown) => {
        if (alive) setLoad({ state: 'error', message: errorText(e) });
      },
    );

    return () => {
      alive = false;
      view?.destroy();
      viewRef.current = null;
    };
  }, [client, sessionId, path, reloads]);

  // The diff base follows `change` / `baseRev` without reloading the file (no change: no base, see `base` above).
  useEffect(() => {
    if (!ready) return;
    const was = asked.current;
    if (was.hasChange === hasChange && was.baseRev === baseRev) return;
    asked.current = { hasChange, baseRev };
    if (!hasChange) return;
    let alive = true;
    void readBase(client, sessionId, path, baseRev).then((b) => alive && setFetchedBase(b));
    return () => {
      alive = false;
    };
  }, [ready, hasChange, baseRev, client, sessionId, path]);

  useEffect(() => {
    viewRef.current?.dispatch({ effects: themeC.reconfigure(editorTheme(fontFamily, fontSize)) });
  }, [fontFamily, fontSize, themeTick, ready]);

  // Read-only while the diff is shown (the mock): the merged view is for reading what the agent changed.
  useEffect(() => {
    const on = diff && base !== null;
    viewRef.current?.dispatch({
      effects: diffC.reconfigure(on ? [unifiedMergeView({ original: base, mergeControls: false, gutter: true }), lockedExt] : []),
    });
  }, [diff, base, ready]);

  /** Keep the buffer against the agent's version: the next ⌘S writes over it (the sha it is guarded by becomes the disk's). */
  const keepMine = () => {
    client.readFile(sessionId, path).then(
      (f) => {
        shaRef.current = f.sha;
        setExternal(false);
        setSave((s) => (s === 'conflict' ? 'edited' : s));
      },
      (e: unknown) => {
        setSaveError(errorText(e));
        setSave('error');
      },
    );
  };

  const binary = load.state === 'ready' && load.binary;
  return (
    <div className="flex h-full min-h-0 flex-col bg-term text-txt">
      {change && (
        <div className="flex h-7 shrink-0 items-center gap-2 border-b border-line bg-blue/5 px-3 text-[11px] whitespace-nowrap text-dim">
          <span className="min-w-0 truncate">Changed by {change.by}</span>
          <span className="text-green tabular-nums">+{change.added}</span>
          <span className="text-red tabular-nums">−{change.removed}</span>
          <span className="flex-1" />
          {base !== null && (
            <button
              type="button"
              aria-pressed={diff}
              title="Show what the agent removed"
              onClick={() => setDiff((d) => !d)}
              className={`inline-flex h-5 items-center gap-1 rounded px-1.5 ${diff ? 'bg-sel text-txt' : 'hover:bg-black/5 dark:hover:bg-white/10'}`}
            >
              <GitCompare aria-hidden className="size-3.5" />
              Diff
            </button>
          )}
          {onReview && (
            <button type="button" onClick={onReview} className="inline-flex h-5 items-center gap-1 rounded px-1.5 text-blue hover:bg-sel">
              Review <span className="opacity-60">⇧⌘R</span>
            </button>
          )}
        </div>
      )}
      {load.state === 'loading' && <div className="px-3 py-2 text-[12px] text-tdim">Loading…</div>}
      {load.state === 'error' && (
        <div role="alert" className="px-3 py-2 text-[12px] text-red">
          {load.message}
        </div>
      )}
      {binary && <div className="px-3 py-2 text-[12px] text-tdim">Binary file · {formatSize(load.size)} · read-only</div>}
      <div ref={hostRef} className={`min-h-0 flex-1 overflow-hidden ${ready && !binary ? '' : 'hidden'}`} />
      <div className="flex h-6 shrink-0 items-center gap-3 border-t border-line px-3 text-[11px] whitespace-nowrap text-dim">
        {load.state === 'ready' && !load.binary && (
          <>
            <span>{languageFor(path).name}</span>
            <span>{load.indent}</span>
            <span>{`UTF-8 · ${load.eol}`}</span>
          </>
        )}
        <span className="flex-1" />
        {ready && (
          <span className="min-w-0 truncate">
            {binary ? (
              'Read-only'
            ) : diff ? (
              'Read-only while diff is shown'
            ) : external ? (
              <>
                <span className="text-orange">Changed on disk by the agent</span> ·{' '}
                <button type="button" onClick={reload} className="text-blue hover:underline">
                  Reload
                </button>{' '}
                ·{' '}
                <button type="button" onClick={keepMine} className="text-blue hover:underline">
                  Keep mine
                </button>
              </>
            ) : save === 'saved' ? (
              'Saved'
            ) : save === 'edited' ? (
              'Edited · ⌘S to save'
            ) : save === 'saving' ? (
              'Saving…'
            ) : save === 'conflict' ? (
              <>
                <span className="text-orange">Changed on disk</span> ·{' '}
                <button type="button" onClick={reload} className="text-blue hover:underline">
                  Reload
                </button>
              </>
            ) : (
              <span className="text-red">Save failed: {saveError}</span>
            )}
          </span>
        )}
      </div>
    </div>
  );
}

function formatSize(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`;
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`;
  return `${(bytes / 1024 / 1024).toFixed(1)} MB`;
}
