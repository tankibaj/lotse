import { Check, Copy, FolderGit2, GitBranch } from 'lucide-react';
import { useEffect, useMemo, useRef, useState } from 'react';
import type { ReactNode } from 'react';
import { useClient } from '../../lib/client';
import type { DiffFile, DiffStatus, GitDiff, GitInfo, Review, SessionId } from '../../lib/client/types';
import { useCoarseNow } from '../../lib/state/clock';
import { coarseAge, shortBranch } from '../../lib/state/selectors';
import { openReview } from '../../lib/ui/panel';
import { useSessions } from '../../panes/useSessions';
import { NoGitIcon } from '../../sidebar/NoGit';
import { openExternal } from '../../terminal/links';
import { PR_DOT, mergeCommand, prLabel, prTitle } from './pr';

// Changes panel (⇧⌘G), spec/mock.html `changesPanel`: files with uncommitted changes (each opens the review at that
// file), Review full diff, the commits ahead of the base and the `git merge` command for the main checkout.

const BADGE: Record<DiffStatus, { letter: string; color: string; label: string }> = {
  modified: { letter: 'M', color: 'var(--dim)', label: 'Modified' },
  added: { letter: 'A', color: 'var(--green)', label: 'Added' },
  deleted: { letter: 'D', color: 'var(--red)', label: 'Deleted' },
  renamed: { letter: 'R', color: 'var(--blue)', label: 'Renamed' },
  untracked: { letter: 'A', color: 'var(--green)', label: 'Untracked' },
};

const sectionLabel = 'mb-1.5 text-[11px] font-semibold uppercase tracking-wide text-dim';
const card = 'overflow-hidden rounded-lg bg-win ring-1 ring-line';

/** Five squares, green for the added share, red for the removed share (the mock's `bar`). */
function ChangeBar({ added, removed }: { added: number; removed: number }) {
  const total = added + removed || 1;
  const green = Math.round((added / total) * 5);
  const red = Math.min(5 - green, Math.round((removed / total) * 5));
  return (
    <span className="mt-1 flex justify-end gap-0.5" aria-hidden="true">
      {Array.from({ length: 5 }, (_, k) => (
        <span
          key={k}
          className="h-1.5 w-1.5 rounded-[2px]"
          style={{ background: k < green ? 'var(--green)' : k < green + red ? 'var(--red)' : 'var(--line)' }}
        />
      ))}
    </span>
  );
}

function FileRow({ file, first, onOpen }: { file: DiffFile; first: boolean; onOpen: (path: string) => void }) {
  const badge = BADGE[file.status];
  const slash = file.path.lastIndexOf('/');
  const name = file.path.slice(slash + 1);
  const dir = file.path.slice(0, slash + 1);
  const added = file.added ?? 0;
  const removed = file.removed ?? 0;
  const title = file.old_path ? `Open diff of ${file.path} (renamed from ${file.old_path})` : `Open diff of ${file.path}`;
  return (
    <button
      type="button"
      data-file={file.path}
      aria-label={`Open diff of ${file.path}`}
      title={title}
      className={`group flex w-full cursor-pointer items-center gap-2.5 px-2.5 py-2 text-left outline-none hover:bg-sel focus-visible:bg-sel focus-visible:ring-2 focus-visible:ring-inset focus-visible:ring-blue ${first ? '' : 'border-t'}`}
      onClick={() => onOpen(file.path)}
    >
      <span
        className="grid h-4 w-4 shrink-0 place-items-center rounded text-[10px] font-semibold"
        style={{ color: badge.color, background: `color-mix(in srgb, ${badge.color} 14%, transparent)` }}
        role="img"
        aria-label={badge.label}
        title={badge.label}
      >
        {badge.letter}
      </span>
      <span className="min-w-0 flex-1">
        <span className="block truncate text-[13px]">{name}</span>
        <span className="block truncate font-mono text-[11px] text-dim">{dir || './'}</span>
      </span>
      <span className="shrink-0 text-right">
        {file.binary ? (
          <span className="block text-[11px] text-dim">binary</span>
        ) : (
          <>
            <span className="block text-[11px] tabular-nums">
              <span className="text-green">+{added}</span>
              {removed > 0 && <span className="text-red"> −{removed}</span>}
            </span>
            <ChangeBar added={added} removed={removed} />
          </>
        )}
      </span>
      <span className="text-[12px] opacity-0 group-hover:opacity-60 group-focus-visible:opacity-60" aria-hidden="true">
        ›
      </span>
    </button>
  );
}

function PrPill({ pr }: { pr: NonNullable<GitInfo['pr']> }) {
  const checks = pr.checks ? ` · checks ${{ pass: 'passing', fail: 'failing', pending: 'pending' }[pr.checks]}` : '';
  return (
    <button
      type="button"
      className="ml-auto inline-flex shrink-0 items-center gap-1 rounded-full bg-line px-1.5 py-0.5 text-[11px] text-txt hover:brightness-95"
      title={`Open ${prTitle(pr).toLowerCase()}${checks}`}
      aria-label={`Open ${prTitle(pr).toLowerCase()}`}
      onClick={() => openExternal(pr.url)}
    >
      <span className={`h-1.5 w-1.5 rounded-full ${PR_DOT[pr.state]}`} aria-hidden="true" />
      {prLabel(pr)}
    </button>
  );
}

function MergeCommand({ branch }: { branch: string }) {
  const [copied, setCopied] = useState(false);
  const timer = useRef<ReturnType<typeof setTimeout>>(undefined);
  useEffect(() => () => clearTimeout(timer.current), []);
  const command = mergeCommand(branch);
  const copy = () => {
    void navigator.clipboard?.writeText(command).then(
      () => {
        setCopied(true);
        clearTimeout(timer.current);
        timer.current = setTimeout(() => setCopied(false), 1500);
      },
      () => {},
    );
  };
  return (
    <>
      <div className={`mt-5 ${sectionLabel}`}>Merge</div>
      <div className={`${card} flex items-center gap-2 py-1 pl-2.5 pr-1`}>
        <code className="min-w-0 flex-1 truncate font-mono text-[12px]" title={command}>
          {command}
        </code>
        <button
          type="button"
          aria-label="Copy merge command"
          title="Copy"
          className="inline-flex h-6 shrink-0 cursor-pointer items-center gap-1 rounded-md px-1.5 text-[11px] text-dim hover:bg-sel hover:text-txt focus-visible:ring-2 focus-visible:ring-blue"
          onClick={copy}
        >
          {copied ? <Check size={12} strokeWidth={2} aria-hidden="true" /> : <Copy size={12} strokeWidth={2} aria-hidden="true" />}
          {copied && <span role="status">Copied</span>}
        </button>
      </div>
      <div className="mt-1 text-[11px] text-dim">Run it in the main checkout, for example in a Lotse terminal.</div>
    </>
  );
}

function Empty({ children }: { children: ReactNode }) {
  return <div className="rounded-lg bg-win p-4 text-center text-[12px] text-dim">{children}</div>;
}

/** `at`: when the answer arrived; commit ages never count from before it (the coarse clock ticks every 30 s). */
type Load = { status: 'loading' } | { status: 'error'; message: string } | { status: 'ready'; diff: GitDiff; review: Review; at: number };

function ChangesBody({ sessionId }: { sessionId: SessionId }) {
  const client = useClient();
  const { sessions, projects, loaded } = useSessions();
  const session = sessions.find((s) => s.id === sessionId);
  const project = projects.find((p) => p.id === session?.project_id);
  const git = session?.git;
  const isGit = !!project?.is_git && !!git?.is_git;
  // Whatever the daemon pushes about the checkout (`git_info_changed`) means the lists may be stale.
  const gitKey = git ? [git.branch, git.ahead, git.behind, git.dirty, git.added, git.removed].join('|') : '';
  const [load, setLoad] = useState<Load>({ status: 'loading' });
  const [attempt, setAttempt] = useState(0);

  useEffect(() => {
    if (!isGit) return;
    let stale = false;
    void Promise.all([client.getDiff(sessionId), client.getReview(sessionId)]).then(
      ([diff, review]) => !stale && setLoad({ status: 'ready', diff, review, at: Date.now() }),
      (e: unknown) => !stale && setLoad({ status: 'error', message: e instanceof Error ? e.message : 'Something went wrong' }),
    );
    return () => {
      stale = true;
    };
  }, [client, sessionId, isGit, gitKey, attempt]);

  const files = useMemo(() => (load.status === 'ready' ? load.diff.files : []), [load]);
  const commits = load.status === 'ready' ? load.review.commits : [];
  const tick = useCoarseNow((t) => commits.map((c) => coarseAge(t - c.at)).join('|'), () => false, 30_000);
  const now = load.status === 'ready' ? Math.max(tick, load.at) : tick;
  const totals = useMemo(
    () => files.reduce((t, f) => ({ added: t.added + (f.added ?? 0), removed: t.removed + (f.removed ?? 0) }), { added: 0, removed: 0 }),
    [files],
  );

  if (!session) return <div className="text-[13px] text-dim">{loaded ? 'This session is gone.' : 'Loading…'}</div>;
  if (!isGit) {
    return (
      <>
        <h3 className="mb-2 font-semibold">Changes</h3>
        <Empty>
          <span className="mx-auto flex w-fit text-dim">
            <NoGitIcon size={16} />
          </span>
          <div className="mt-2">{project?.name ?? 'This folder'} is not a git repository.</div>
        </Empty>
      </>
    );
  }
  if (load.status === 'error') {
    return (
      <div role="alert" className="text-[12px] text-dim">
        <h3 className="mb-2 text-[13px] font-semibold text-txt">Changes</h3>
        Could not load the changes: {load.message}{' '}
        <button type="button" className="cursor-pointer text-blue hover:underline" onClick={() => setAttempt((n) => n + 1)}>
          Retry
        </button>
      </div>
    );
  }
  if (load.status === 'loading') return <div className="text-[13px] text-dim">Loading…</div>;

  const { review } = load;
  const base = review.base.replace(/^origin\//, '');
  const Icon = git?.worktree ? FolderGit2 : GitBranch;
  const open = (file?: string) => openReview(sessionId, file);
  return (
    <>
      <div className="flex items-baseline gap-2">
        <h3 className="font-semibold">Changes</h3>
        <span
          className="flex flex-1 items-baseline gap-2"
          role="group"
          aria-label={`${files.length} changed ${files.length === 1 ? 'file' : 'files'}, ${totals.added} added, ${totals.removed} removed`}
        >
          <span className="rounded-full bg-line px-1.5 text-[11px]">{files.length}</span>
          <span className="ml-auto text-[12px] tabular-nums">
            <span className="text-green">+{totals.added}</span> <span className="text-red">−{totals.removed}</span>
          </span>
        </span>
      </div>
      <div className="mt-1.5 flex min-w-0 items-center gap-1.5 text-[12px] text-dim">
        <Icon size={12} strokeWidth={2} className="flex-none" aria-hidden="true" />
        <span className="min-w-0 truncate font-mono" title={git?.branch}>
          {shortBranch(git?.branch || 'HEAD')}
        </span>
        {review.ahead > 0 && <span className="tabular-nums" title={`${review.ahead} commit${review.ahead === 1 ? '' : 's'} ahead of ${base}`}>↑{review.ahead}</span>}
        {review.behind > 0 && <span className="tabular-nums" title={`${review.behind} commit${review.behind === 1 ? '' : 's'} behind ${base}`}>↓{review.behind}</span>}
        {git?.pr && <PrPill pr={git.pr} />}
      </div>

      {files.length === 0 ? (
        <div className="mt-4">
          <Empty>No changes</Empty>
        </div>
      ) : (
        <>
          <div className={`mt-4 ${sectionLabel}`}>Uncommitted</div>
          <div className={card}>
            {files.map((f, k) => (
              <FileRow key={f.path} file={f} first={k === 0} onOpen={open} />
            ))}
          </div>
          <button
            type="button"
            className="mt-3 flex w-full cursor-pointer items-center justify-center gap-2 rounded-lg bg-win py-1.5 text-[13px] font-medium text-blue ring-1 ring-line hover:bg-sel focus-visible:ring-2 focus-visible:ring-blue"
            onClick={() => open()}
          >
            Review full diff <span className="text-[11px] opacity-60">⌥⌘D</span>
          </button>
        </>
      )}

      {review.commits.length > 0 && (
        <>
          <div className={`mt-5 flex ${sectionLabel}`}>
            Commits ahead of {base}
            <span className="ml-auto font-normal normal-case">{review.ahead}</span>
          </div>
          <div className="space-y-1.5">
            {review.commits.map((c) => (
              <div key={c.sha} className="flex items-baseline gap-2 text-[12px]" title={c.author}>
                <span className="font-mono text-[11px] text-dim">{c.sha.slice(0, 7)}</span>
                <span className="flex-1 truncate">{c.subject}</span>
                <span className="shrink-0 text-[11px] text-dim">{coarseAge(now - c.at)}</span>
              </div>
            ))}
            {review.truncated && <div className="text-[11px] text-dim">and more</div>}
          </div>
        </>
      )}

      {git?.worktree && review.ahead > 0 && git.branch && <MergeCommand branch={git.branch} />}
    </>
  );
}

/** Mounted by `RightPanel` with the selected session. The body is keyed by session so no state outlives a switch. */
export function ChangesPanel({ sessionId }: { sessionId: SessionId | null }) {
  return (
    <div data-testid="panel-changes" data-session-id={sessionId ?? undefined} className="text-[13px]">
      {sessionId ? <ChangesBody key={sessionId} sessionId={sessionId} /> : <div className="text-dim">No session selected.</div>}
    </div>
  );
}
