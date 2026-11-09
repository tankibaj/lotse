import { defaultKeymap } from '@codemirror/commands';
import { MergeView, unifiedMergeView } from '@codemirror/merge';
import { EditorState, Prec } from '@codemirror/state';
import type { Extension } from '@codemirror/state';
import { EditorView, keymap, lineNumbers } from '@codemirror/view';
import { useEffect, useMemo, useRef, useState } from 'react';
import { languageFor } from '../editor/support';
import { editorTheme } from '../editor/theme';
import { DaemonError, useClient } from '../lib/client';
import type { DiffFile, DiffStatus, SessionId } from '../lib/client/types';
import { diffLines } from '../sheets/diffLines';
import type { DiffLineKind } from '../sheets/diffLines';
import { useTerminalAppearance } from '../terminal/useTerminalAppearance';
import { commentExtension, setComments } from './commentExtension';
import type { CommentHandlers } from './commentExtension';
import { getComments, putComment, removeComment, useComments } from './comments';

export type Layout = 'split' | 'unified';

export const BADGE: Record<DiffStatus, { letter: string; tone: string; label: string }> = {
  modified: { letter: 'M', tone: 'text-orange', label: 'Modified' },
  added: { letter: 'A', tone: 'text-green', label: 'Added' },
  deleted: { letter: 'D', tone: 'text-red', label: 'Deleted' },
  renamed: { letter: 'R', tone: 'text-blue', label: 'Renamed' },
  untracked: { letter: 'U', tone: 'text-dim', label: 'Untracked' },
};

const LINE: Record<DiffLineKind, string> = {
  meta: 'text-dim',
  hunk: 'text-blue',
  add: 'text-green [background:color-mix(in_srgb,var(--green)_13%,transparent)]',
  del: 'text-red [background:color-mix(in_srgb,var(--red)_12%,transparent)]',
  ctx: '',
};

type Load = { state: 'loading' } | { state: 'ready'; base: string; work: string } | { state: 'binary' } | { state: 'patch'; reason: string };

const COLLAPSE = { margin: 3, minSize: 4 };

// Whole files side by side, so the page is the scroller (no inner height); unchanged stretches fold into one row like
// the mock's hunk rows.
const layoutTheme = EditorView.theme({
  '&': { height: 'auto' },
  '.cm-collapsedLines': {
    backgroundColor: 'color-mix(in srgb, var(--blue) 6%, transparent)',
    backgroundImage: 'none',
    color: 'var(--tdim)',
    padding: '2px 16px',
  },
});

/** The file as the merge base had it (a file new on the branch had none) and as it is now. */
async function readTexts(client: ReturnType<typeof useClient>, sessionId: SessionId, file: Pick<DiffFile, 'path' | 'old_path' | 'status'>, baseRev: string): Promise<Load> {
  const missing = (e: unknown) => (e instanceof DaemonError && e.code === 'not_found' ? null : Promise.reject(e instanceof Error ? e : new Error(String(e))));
  const [base, work] = await Promise.all([
    file.status === 'added' || file.status === 'untracked' ? null : client.readFile(sessionId, file.old_path ?? file.path, baseRev).catch(missing),
    file.status === 'deleted' ? null : client.readFile(sessionId, file.path),
  ]);
  if (base?.binary || work?.binary) return { state: 'binary' };
  return { state: 'ready', base: base?.text ?? '', work: work?.text ?? '' };
}

/** Light, dark and auto are the `dark` class on <html>: a change rebuilds the views with the new colours. */
function useThemeTick(): number {
  const [tick, setTick] = useState(0);
  useEffect(() => {
    const obs = new MutationObserver(() => setTick((n) => n + 1));
    obs.observe(document.documentElement, { attributes: true, attributeFilter: ['class'] });
    return () => obs.disconnect();
  }, []);
  return tick;
}

/** One changed file of the review: a sticky header and the merge view, with line comments on the working-tree side. */
export function FileDiff({ sessionId, file, baseRev, layout }: { sessionId: SessionId; file: DiffFile; baseRev: string; layout: Layout }) {
  const client = useClient();
  const { fontFamily, fontSize } = useTerminalAppearance();
  const themeTick = useThemeTick();
  const hostRef = useRef<HTMLDivElement>(null);
  const viewRef = useRef<EditorView | null>(null);
  const [load, setLoad] = useState<Load>(file.binary ? { state: 'binary' } : { state: 'loading' });
  const all = useComments(sessionId);
  const mine = useMemo(() => all.filter((c) => c.path === file.path), [all, file.path]);
  const mineRef = useRef(mine);
  const { path, old_path: oldPath, status, binary } = file;
  const badge = BADGE[status];

  useEffect(() => {
    if (binary) return;
    let alive = true;
    void readTexts(client, sessionId, { path, old_path: oldPath, status }, baseRev).then(
      (l) => alive && setLoad(l),
      (e: unknown) => alive && setLoad({ state: 'patch', reason: e instanceof Error ? e.message : String(e) }),
    );
    return () => {
      alive = false;
    };
  }, [client, sessionId, path, oldPath, status, binary, baseRev]);

  const handlers = useMemo<CommentHandlers>(
    () => ({
      add(line, quote) {
        if (getComments(sessionId).some((c) => c.path === path && c.line === line)) {
          hostRef.current?.querySelector<HTMLTextAreaElement>(`textarea[aria-label="Comment for line ${line}"]`)?.focus();
          return;
        }
        putComment(sessionId, { path, line, quote, text: '' });
      },
      change(line, text) {
        const current = getComments(sessionId).find((c) => c.path === path && c.line === line);
        if (current) putComment(sessionId, { ...current, text });
      },
      remove: (line) => removeComment(sessionId, path, line),
    }),
    [sessionId, path],
  );

  // Before the views effect: it reads the comments as they are now.
  useEffect(() => {
    mineRef.current = mine;
    viewRef.current?.dispatch({ effects: setComments.of(mine) });
  }, [mine]);

  useEffect(() => {
    const host = hostRef.current;
    if (load.state !== 'ready' || !host) return;
    const common: Extension[] = [
      lineNumbers(),
      EditorState.readOnly.of(true), // focusable, so the cursor can pick a line for ⌘Return
      keymap.of(defaultKeymap),
      EditorView.contentAttributes.of({ 'aria-label': `Review ${path}: ⌘Return adds a comment on the cursor line` }),
      languageFor(path).support ?? [],
      editorTheme(fontFamily, fontSize),
      layoutTheme,
    ];
    const withComments = [...common, Prec.high(commentExtension(handlers))];
    let target: EditorView;
    let destroy: () => void;
    if (layout === 'split') {
      const merge = new MergeView({
        a: { doc: load.base, extensions: common },
        b: { doc: load.work, extensions: withComments },
        parent: host,
        gutter: true,
        collapseUnchanged: COLLAPSE,
      });
      target = merge.b;
      destroy = () => merge.destroy();
    } else {
      const view = new EditorView({
        doc: load.work,
        extensions: [...withComments, unifiedMergeView({ original: load.base, mergeControls: false, gutter: true, collapseUnchanged: COLLAPSE })],
        parent: host,
      });
      target = view;
      destroy = () => view.destroy();
    }
    viewRef.current = target;
    target.dispatch({ effects: setComments.of(mineRef.current) });
    return () => {
      viewRef.current = null;
      destroy();
    };
  }, [load, layout, path, handlers, fontFamily, fontSize, themeTick]);

  const lines = useMemo(() => (load.state === 'patch' ? diffLines(file.diff) : []), [load.state, file.diff]);

  return (
    <section data-file={path} className="border-b last:border-b-0">
      <div className="sticky top-0 z-10 flex items-center gap-2 border-b bg-side px-4 py-2 font-sans text-[12px] font-medium">
        <span className={badge.tone} title={badge.label} aria-label={badge.label}>
          {badge.letter}
        </span>
        <span className="min-w-0 truncate font-mono" title={path}>
          {path}
          {oldPath && <span className="text-dim"> ← {oldPath}</span>}
        </span>
        {file.truncated && <span className="text-[11px] font-normal text-orange">cut</span>}
      </div>
      {file.truncated && <div className="px-4 pt-1 font-sans text-[11px] text-orange">The diff text of this file is cut (size cap): Copy patch is incomplete for it.</div>}
      {load.state === 'loading' && <div className="px-4 py-3 text-[12px] text-dim">Loading…</div>}
      {load.state === 'binary' && <div className="px-4 py-3 text-[12px] text-dim">Binary file: not shown.</div>}
      {load.state === 'patch' && (
        <>
          <div role="status" className="px-4 pt-2 text-[11px] text-orange">
            Showing the patch text, comments are off for this file: {load.reason}
          </div>
          <div className="overflow-x-auto py-1.5 font-mono text-[12.5px] leading-[1.55]">
            <div className="w-max min-w-full">
              {lines.map((l, i) => (
                <div key={i} data-kind={l.kind} className={`px-4 whitespace-pre ${LINE[l.kind]}`}>
                  {l.text === '' ? ' ' : l.text}
                </div>
              ))}
            </div>
          </div>
        </>
      )}
      <div ref={hostRef} className={load.state === 'ready' ? '' : 'hidden'} />
    </section>
  );
}
