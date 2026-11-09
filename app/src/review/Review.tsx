import { FolderGit2 } from 'lucide-react';
import { useEffect, useRef, useState } from 'react';
import { NotSubmitted } from '../composer/send';
import { useClient } from '../lib/client';
import type { DiffFile, GitDiff, Review as BranchReview, SessionId } from '../lib/client/types';
import { useSessions } from '../panes/useSessions';
import { diffCounts } from '../sheets/diffLines';
import { inTauri } from '../sheets/hostBridge';
import { useDialogFocus } from '../sheets/ui';
import { buildReviewPrompt, clearComments, dropEmptyComments, useComments, written } from './comments';
import { BADGE, FileDiff } from './FileDiff';
import type { Layout } from './FileDiff';
import { sendReviewPrompt } from './send';

// The mock's `#diff`: the whole change of a session against its review base, file by file, with line comments that go
// to the agent as one prompt. The shell mounts it in the `review` slot and closes it on Esc.

// `base` is what the branch is compared with (shown); `oldRev` is where the old side of every file is read: the merge base.
type Load = { status: 'loading' } | { status: 'error'; message: string } | { status: 'ready'; diff: GitDiff; base: string; oldRev: string };

/**
 * The file list, counts and patch come from `getDiff(…, 'merge_base')`, so the old side must be the merge base too, not
 * the base tip (they differ when the branch is behind). The protocol has no merge-base field: it is the parent of the
 * oldest commit ahead, HEAD when none is ahead. A truncated commit list cannot tell, so that case reads at the base.
 */
function mergeBaseRev(r: BranchReview): string {
  if (r.commits.length === 0) return 'HEAD';
  const oldest = r.commits[r.commits.length - 1];
  return r.truncated || !oldest ? r.base : `${oldest.sha}^`;
}

const PR_STATE = { open: 'Open', draft: 'Draft', merged: 'Merged', closed: 'Closed' } as const;
const errorText = (e: unknown) => (e instanceof Error ? e.message : String(e));
const counts = (f: DiffFile) => (f.added !== undefined && f.removed !== undefined ? { added: f.added, removed: f.removed } : diffCounts(f.diff));
const toolbarBtn = 'rounded-md bg-black/5 px-2 py-1 disabled:opacity-50 enabled:hover:bg-black/10 dark:bg-white/10 dark:enabled:hover:bg-white/15';

export function Review({ sessionId, file, onClose }: { sessionId: SessionId; file?: string; onClose: () => void }) {
  const client = useClient();
  const box = useRef<HTMLDivElement>(null);
  const body = useRef<HTMLDivElement>(null);
  const trap = useDialogFocus(box);
  const { sessions } = useSessions();
  const session = sessions.find((s) => s.id === sessionId);
  const title = session?.title || 'Session';
  const [load, setLoad] = useState<Load>({ status: 'loading' });
  const [layout, setLayout] = useState<Layout>('split');
  const [selected, setSelected] = useState<string | null>(file ?? null);
  const [copy, setCopy] = useState<'idle' | 'copied' | 'failed'>('idle');
  const [sending, setSending] = useState(false);
  const [problem, setProblem] = useState('');
  const [notice, setNotice] = useState('');
  const comments = written(useComments(sessionId));

  useEffect(() => {
    let alive = true;
    void (async () => {
      const diff = await client.getDiff(sessionId, 'merge_base');
      // what the base is called: the files are read at it
      const review = diff.is_git && diff.files.length > 0 ? await client.getReview(sessionId) : null;
      if (alive) setLoad({ status: 'ready', diff, base: review?.base ?? 'HEAD', oldRev: review ? mergeBaseRev(review) : 'HEAD' });
    })().catch((e: unknown) => alive && setLoad({ status: 'error', message: errorText(e) }));
    return () => {
      alive = false;
    };
  }, [client, sessionId]);

  // Boxes opened and left empty are not comments.
  useEffect(() => () => dropEmptyComments(sessionId), [sessionId]);

  useEffect(() => {
    if (copy === 'idle') return;
    const t = setTimeout(() => setCopy('idle'), 1500);
    return () => clearTimeout(t);
  }, [copy]);

  const files = load.status === 'ready' ? load.diff.files : [];
  const current = files.find((f) => f.path === selected);
  const shown = current ? [current] : files;
  const patch = shown
    .map((f) => f.diff)
    .filter((d) => d !== '')
    .map((d) => (d.endsWith('\n') ? d : `${d}\n`))
    .join('');
  const total = files.reduce((t, f) => ({ added: t.added + counts(f).added, removed: t.removed + counts(f).removed }), { added: 0, removed: 0 });

  const select = (path: string | null) => {
    setSelected(path);
    if (body.current) body.current.scrollTop = 0;
  };

  const copyPatch = () => {
    navigator.clipboard.writeText(patch).then(
      () => setCopy('copied'),
      () => setCopy('failed'),
    );
  };

  const sendReview = async () => {
    setSending(true);
    setProblem('');
    setNotice('');
    try {
      await sendReviewPrompt(client, sessionId, buildReviewPrompt(comments));
      clearComments(sessionId);
      onClose();
    } catch (e) {
      if (e instanceof NotSubmitted) {
        // the words are in the agent's input box: the comments are spent, the Return is the user's
        clearComments(sessionId);
        setNotice('The review is in the agent’s input but was not submitted: press Return in its terminal.');
      } else {
        setProblem(`Could not send the review: ${errorText(e)}`);
      }
    } finally {
      setSending(false);
    }
  };

  const pr = session?.git.pr;
  return (
    <div ref={box} onKeyDown={trap} className="absolute inset-0 z-30 flex flex-col bg-win text-txt" role="dialog" aria-modal="true" aria-label="Review">
      <div className="flex h-11 shrink-0 items-center gap-3 border-b px-3">
        {inTauri() ? (
          <span className="w-[52px] shrink-0" aria-hidden="true" />
        ) : (
          <>
            <span className="size-3 rounded-full bg-[#FF5F57]" />
            <span className="size-3 rounded-full bg-[#FEBC2E]" />
            <span className="size-3 rounded-full bg-[#28C840]" />
          </>
        )}
        <button type="button" data-autofocus className="ml-2 rounded-md px-2 py-1 text-[13px] text-blue hover:bg-black/5 dark:hover:bg-white/5" onClick={onClose}>
          ‹ Back <span className="text-[11px] text-tdim">esc</span>
        </button>
        <span className="truncate text-[13px] font-medium">
          {title}
          {file ? ` · ${file}` : ''}
        </span>
        <div className="ml-auto flex items-center gap-2 text-[12px]">
          <div className="inline-flex rounded-md bg-line p-0.5" role="group" aria-label="Diff layout">
            {(['split', 'unified'] as const).map((l) => (
              <button key={l} type="button" aria-pressed={layout === l} onClick={() => setLayout(l)} className={`rounded px-2 py-0.5 ${layout === l ? 'bg-sel text-blue' : ''}`}>
                {l === 'split' ? 'Side by side' : 'Unified'}
              </button>
            ))}
          </div>
          {copy !== 'idle' && (
            <span role="status" className="text-dim">
              {copy === 'copied' ? 'Patch copied' : 'Could not copy the patch'}
            </span>
          )}
          <button type="button" className={toolbarBtn} disabled={patch === ''} onClick={copyPatch}>
            Copy patch
          </button>
          <button
            type="button"
            className="rounded-md bg-blue px-2.5 py-1 text-white disabled:opacity-50"
            disabled={comments.length === 0 || sending}
            title={comments.length === 0 ? 'Add a comment first: click the + beside a line, or press ⌘Return on it' : undefined}
            onClick={() => void sendReview()}
          >
            Send review to agent {comments.length > 0 && <span className="opacity-80">({comments.length})</span>}
          </button>
        </div>
      </div>
      {problem && (
        <div role="alert" className="shrink-0 border-b px-4 py-1.5 text-[12px] text-red">
          {problem}
        </div>
      )}
      {notice && (
        <div role="status" className="shrink-0 border-b px-4 py-1.5 text-[12px] text-orange">
          {notice}
        </div>
      )}
      <div className="flex min-h-0 flex-1">
        <aside aria-label="Changed files" className="w-[260px] shrink-0 overflow-y-auto border-r bg-side py-2">
          {load.status === 'ready' && load.diff.is_git && files.length > 0 && (
            <>
              <div className="px-3 pb-2 text-[11px] text-dim">
                {session?.git.branch && (
                  <div className="flex items-center gap-1 font-mono">
                    <FolderGit2 size={13} strokeWidth={1.6} aria-hidden="true" className="shrink-0" />
                    <span className="truncate">{session.git.branch}</span>
                  </div>
                )}
                <div className="mt-0.5">
                  vs <span className="font-mono">{load.base}</span> · ↑{session?.git.ahead ?? 0} · {files.length} file{files.length === 1 ? '' : 's'} ·{' '}
                  <span className="text-green">+{total.added}</span> <span className="text-red">−{total.removed}</span>
                </div>
                {pr && (
                  <div className="mt-1">
                    <span className="rounded bg-line px-1.5 py-0.5 text-[10px]">
                      PR #{pr.number} · {PR_STATE[pr.state]}
                    </span>
                  </div>
                )}
              </div>
              <div className="space-y-0.5 px-1.5">
                <button type="button" aria-current={current ? undefined : 'true'} onClick={() => select(null)} className={`mb-1 flex w-full items-center gap-2 rounded-md px-2 py-1 text-left text-[12px] ${current ? '' : 'bg-sel'}`}>
                  <span className="w-3" />
                  <span className="flex-1">All files</span>
                  <span className="text-tdim tabular-nums">{files.length}</span>
                </button>
                {files.map((f) => {
                  const { added, removed } = counts(f);
                  const badge = BADGE[f.status];
                  return (
                    <button
                      key={f.path}
                      type="button"
                      aria-current={f.path === current?.path ? 'true' : undefined}
                      title={f.path}
                      onClick={() => select(f.path)}
                      className={`flex w-full items-center gap-2 rounded-md px-2 py-1 text-left text-[12px] ${f.path === current?.path ? 'bg-sel' : 'hover:bg-black/5 dark:hover:bg-white/5'}`}
                    >
                      <span className={`w-3 text-[10px] font-semibold ${badge.tone}`} aria-label={badge.label}>
                        {badge.letter}
                      </span>
                      <span className="flex-1 truncate font-mono">{f.path.split('/').pop()}</span>
                      {(added > 0 || removed === 0) && !f.binary && <span className="text-green tabular-nums">+{added}</span>}
                      {removed > 0 && <span className="text-red tabular-nums">−{removed}</span>}
                    </button>
                  );
                })}
              </div>
            </>
          )}
        </aside>
        <div ref={body} className="min-w-0 flex-1 overflow-auto font-mono text-[12.5px] leading-[1.55]">
          {load.status === 'loading' && <div className="px-4 py-6 text-center font-sans text-dim">Loading changes…</div>}
          {load.status === 'error' && (
            <div role="alert" className="px-4 py-6 text-center font-sans text-red">
              {load.message}
            </div>
          )}
          {load.status === 'ready' && !load.diff.is_git && <div className="px-4 py-6 text-center font-sans text-dim">Not a git repository: nothing to review.</div>}
          {load.status === 'ready' && load.diff.is_git && files.length === 0 && <div className="px-4 py-6 text-center font-sans text-dim">No changes to review</div>}
          {load.status === 'ready' && shown.map((f) => <FileDiff key={f.path} sessionId={sessionId} file={f} baseRev={load.oldRev} layout={layout} />)}
          {load.status === 'ready' && load.diff.truncated && (
            <div role="status" className="border-t px-4 py-2 font-sans text-[12px] text-orange">
              The diff is cut at 512 KB: later files are left out. Run <span className="font-mono">git diff</span> in the terminal for the rest.
            </div>
          )}
        </div>
      </div>
    </div>
  );
}
