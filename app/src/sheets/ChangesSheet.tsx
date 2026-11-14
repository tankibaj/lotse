import { ChevronRight } from 'lucide-react';
import { useMemo, useState } from 'react';
import { useClient } from '../lib/client';
import type { DiffFile, DiffStatus, SessionId } from '../lib/client/types';
import { useWorkspace } from '../lib/state';
import { diffCounts, diffLines } from './diffLines';
import type { DiffLineKind } from './diffLines';
import { closeOverlay } from './overlay';
import { Modal, btnCls } from './ui';
import { useLoad } from './useLoad';

// Show Changes (session menu): the session folder's `git diff` against HEAD, staged and unstaged together,
// plus the names of untracked files. Read-only; the daemon caps the size (512 KB) and notes binary files.

const BADGE: Record<DiffStatus, { letter: string; tone: string; label: string }> = {
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
  del: 'text-red [background:color-mix(in_srgb,var(--red)_13%,transparent)]',
  ctx: '',
};

/** The first files open, the rest are one click away. */
const OPEN_FIRST = 3;

function FileDiff({ file, open, onToggle }: { file: DiffFile; open: boolean; onToggle: () => void }) {
  const badge = BADGE[file.status];
  const lines = useMemo(() => (open ? diffLines(file.diff) : []), [open, file.diff]);
  const { added, removed } = useMemo(() => diffCounts(file.diff), [file.diff]);
  const hasBody = file.binary || file.status === 'untracked' || file.diff !== '';
  return (
    <section data-file={file.path} className="border-b last:border-b-0">
      <button
        type="button"
        aria-expanded={hasBody ? open : undefined}
        disabled={!hasBody}
        onClick={onToggle}
        className="flex w-full items-center gap-2 px-4 py-1.5 text-left enabled:hover:bg-sel"
      >
        <ChevronRight size={12} strokeWidth={1.8} aria-hidden="true" className={`shrink-0 transition-transform ${hasBody ? 'opacity-60' : 'opacity-0'} ${open ? 'rotate-90' : ''}`} />
        <span className={`w-4 shrink-0 text-center font-mono text-[11px] font-semibold ${badge.tone}`} title={badge.label} aria-label={badge.label}>
          {badge.letter}
        </span>
        <span className="min-w-0 flex-1 truncate font-mono text-[12px]" title={file.path}>
          {file.path}
          {file.old_path && <span className="text-dim"> ← {file.old_path}</span>}
        </span>
        {file.binary && <span className="text-[11px] text-dim">binary</span>}
        {file.truncated && <span className="text-[11px] text-orange">cut</span>}
        {(added > 0 || removed > 0) && (
          <span className="font-mono text-[11px] tabular-nums">
            <span className="text-green">+{added}</span> <span className="text-red">−{removed}</span>
          </span>
        )}
      </button>
      {open && file.binary && <div className="px-10 pb-2 text-[12px] text-dim">Binary file: not shown.</div>}
      {open && file.status === 'untracked' && <div className="px-10 pb-2 text-[12px] text-dim">Not tracked by git: listed by name only.</div>}
      {open && lines.length > 0 && (
        <div role="region" aria-label={`Diff of ${file.path}`} className="overflow-x-auto pb-1.5 font-mono text-[12px] leading-[1.5]">
          <div className="w-max min-w-full">
            {lines.map((l, i) => (
              <div key={i} data-kind={l.kind} className={`px-4 whitespace-pre ${LINE[l.kind]}`}>
                {l.text === '' ? ' ' : l.text}
              </div>
            ))}
          </div>
          {file.truncated && <div className="px-4 pt-1 font-sans text-[11px] text-orange">The rest of this file's diff is cut (size cap).</div>}
        </div>
      )}
    </section>
  );
}

/** Show Changes sheet for `sessionId`. */
export function ChangesSheet({ sessionId }: { sessionId: SessionId }) {
  const client = useClient();
  const { sessions } = useWorkspace();
  const session = sessions.find((s) => s.id === sessionId);
  const [state, reload] = useLoad(() => client.getDiff(sessionId));
  const [toggled, setToggled] = useState<ReadonlyMap<string, boolean>>(new Map());
  const [all, setAll] = useState<boolean | null>(null);

  const files = state.status === 'ready' ? state.data.files : [];
  const isOpen = (f: DiffFile, i: number) => toggled.get(f.path) ?? all ?? i < OPEN_FIRST;
  const flip = (f: DiffFile, i: number) => setToggled(new Map(toggled).set(f.path, !isOpen(f, i)));
  const setEvery = (open: boolean) => {
    setAll(open);
    setToggled(new Map());
  };
  const tracked = files.filter((f) => f.status !== 'untracked').length;
  const untracked = files.length - tracked;

  return (
    <Modal label="Changes" onClose={closeOverlay} panelClass="relative mt-12 mb-8 w-[880px] rounded-xl text-[13px]">
      <div className="flex items-center gap-3 border-b px-4 py-3">
        <h3 className="font-semibold">Changes</h3>
        <span className="min-w-0 flex-1 truncate text-dim" title={session?.title}>
          {session?.title ?? ''}
          {session?.git.is_git && session.git.branch !== '' && <span className="font-mono"> · {session.git.branch}</span>}
        </span>
        <button type="button" className={btnCls} onClick={reload} disabled={state.status === 'loading'}>
          Refresh
        </button>
        <button type="button" data-autofocus onClick={closeOverlay} className="grid h-[26px] w-7 place-items-center rounded-md text-dim hover:bg-sel" aria-label="Close">
          ✕
        </button>
      </div>

      <div className="max-h-[68vh] overflow-y-auto" aria-live="polite">
        {state.status === 'loading' && <div className="px-4 py-6 text-center text-dim">Loading changes…</div>}
        {state.status === 'error' && (
          <div role="alert" className="px-4 py-6 text-center text-red">
            {state.message}
          </div>
        )}
        {state.status === 'ready' && !state.data.is_git && <div className="px-4 py-6 text-center text-dim">No git: this folder is not a git repository.</div>}
        {state.status === 'ready' && state.data.is_git && files.length === 0 && <div className="px-4 py-6 text-center text-dim">No changes.</div>}
        {state.status === 'ready' && state.data.is_git && files.length > 0 && (
          <>
            <div className="flex items-center gap-3 px-4 py-2 text-[12px] text-dim">
              <span>
                {tracked} changed file{tracked === 1 ? '' : 's'}
                {untracked > 0 && ` · ${untracked} untracked`} · against HEAD, staged and unstaged
              </span>
              <span className="ml-auto flex gap-3">
                <button type="button" className="text-blue" onClick={() => setEvery(true)}>
                  Expand all
                </button>
                <button type="button" className="text-blue" onClick={() => setEvery(false)}>
                  Collapse all
                </button>
              </span>
            </div>
            <div className="border-t">
              {files.map((f, i) => (
                <FileDiff key={f.path} file={f} open={isOpen(f, i)} onToggle={() => flip(f, i)} />
              ))}
            </div>
            {state.data.truncated && (
              <div role="status" className="border-t px-4 py-2 text-[12px] text-orange">
                The diff is cut at 512 KB: later files are left out. Run <span className="font-mono">git diff</span> in the terminal for the rest.
              </div>
            )}
          </>
        )}
      </div>
    </Modal>
  );
}
