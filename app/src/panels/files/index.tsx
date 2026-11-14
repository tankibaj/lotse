import { Braces, Copy, File, FileCode, FileText, FlaskConical, Folder, FolderGit2, FolderOpen, Search, SquareArrowOutUpRight } from 'lucide-react';
import { useEffect, useMemo, useState } from 'react';
import { useClient } from '../../lib/client/context';
import type { DiffStatus, FileTree, SessionId } from '../../lib/client/types';
import { useWorkspace } from '../../lib/state';
import { emitUiCommand } from '../../lib/ui/commands';
import { buildTree, countFiles, defaultOpen, visibleRows } from './tree';
import type { Mode, Row } from './tree';

const BADGE: Record<DiffStatus, { letter: string; label: string; tone: string }> = {
  added: { letter: 'A', label: 'Added', tone: 'text-green' },
  modified: { letter: 'M', label: 'Modified', tone: 'text-orange' },
  deleted: { letter: 'D', label: 'Deleted', tone: 'text-red' },
  renamed: { letter: 'R', label: 'Renamed', tone: 'text-orange' },
  untracked: { letter: 'U', label: 'Untracked', tone: 'text-green' },
};

const FILE_ICON_CLASS = 'w-4 h-4 shrink-0 text-dim';

function FileGlyph({ name }: { name: string }) {
  if (/\.json$/.test(name)) return <Braces className={FILE_ICON_CLASS} aria-hidden />;
  if (/\.md$/.test(name)) return <FileText className={FILE_ICON_CLASS} aria-hidden />;
  if (/\.spec\./.test(name)) return <FlaskConical className={FILE_ICON_CLASS} aria-hidden />;
  if (/\.(ts|tsx|js)$/.test(name)) return <FileCode className={FILE_ICON_CLASS} aria-hidden />;
  return <File className={FILE_ICON_CLASS} aria-hidden />;
}

const ROW = 'w-full flex items-center gap-1.5 h-[26px] rounded-md hover:bg-black/5 dark:hover:bg-white/5';
const ICON_BUTTON = 'grid place-items-center w-5 h-5 rounded text-dim hover:bg-sel disabled:opacity-40 disabled:hover:bg-transparent';

/** Files panel (⇧⌘F): the session folder as a tree, All / Changed, a filter, and a click that opens the file in the editor. */
export function FilesPanel({ sessionId }: { sessionId: SessionId | null }) {
  if (sessionId === null) return <div data-testid="panel-files" className="text-[13px] text-dim">No session selected.</div>;
  return <FilesBody key={sessionId} sessionId={sessionId} />;
}

function FilesBody({ sessionId }: { sessionId: SessionId }) {
  const client = useClient();
  const git = useWorkspace().sessions.find((s) => s.id === sessionId)?.git;
  const [tree, setTree] = useState<FileTree | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [reconnects, setReconnects] = useState(0);
  const [mode, setMode] = useState<Mode>('all');
  const [query, setQuery] = useState('');
  const [open, setOpen] = useState<Set<string> | null>(null);
  const [selected, setSelected] = useState<string | null>(null);

  // The folder changes while the agent works: reload whenever the session's git summary moves.
  const stamp = git ? `${git.branch}:${git.dirty}:${git.added ?? 0}:${git.removed ?? 0}` : '';
  useEffect(() => {
    let live = true;
    client.listFiles(sessionId).then(
      (t) => {
        if (!live) return;
        setTree(t);
        setError(null);
      },
      (e: unknown) => live && setError(e instanceof Error ? e.message : 'Could not list files'),
    );
    return () => {
      live = false;
    };
  }, [client, sessionId, stamp, reconnects]);
  useEffect(() => client.onConnection((s) => s === 'reconnected' && setReconnects((n) => n + 1)), [client]);

  const nodes = useMemo(() => buildTree(tree?.entries ?? []), [tree]);
  const { files, changed } = useMemo(() => countFiles(nodes), [nodes]);
  const openSet = open ?? defaultOpen(nodes);
  const rows = visibleRows(nodes, tree?.is_git ? mode : 'all', query, openSet);
  const isGit = tree?.is_git === true;

  const toggle = (path: string) => {
    const next = new Set(openSet);
    if (!next.delete(path)) next.add(path);
    setOpen(next);
  };
  const openFile = (path: string) => {
    setSelected(path);
    emitUiCommand('openFile', sessionId, path);
  };

  return (
    <div data-testid="panel-files" data-session-id={sessionId}>
      <div className="flex items-start gap-2 mb-3">
        <div className="flex-1 min-w-0">
          <h3 className="font-semibold">Files</h3>
          {tree && (
            <div className="text-[11px] truncate mt-0.5 text-dim">
              <span className="inline-flex items-center gap-1">
                {git?.worktree ? <FolderGit2 className="w-3 h-3" aria-hidden /> : <Folder className="w-3 h-3" aria-hidden />}
                <span className="font-mono" title={tree.root}>{tree.root.replace(/^\/Users\/[^/]+/, '~')}</span>
              </span>
            </div>
          )}
        </div>
        {isGit && (
          <div className="inline-flex p-0.5 rounded-md bg-black/5 dark:bg-white/10 text-[11px]" role="tablist">
            {([['all', 'All'], ['changed', `Changed ${changed}`]] as const).map(([key, label]) => (
              <button
                key={key}
                role="tab"
                aria-selected={mode === key}
                onClick={() => setMode(key)}
                className={`px-2 py-0.5 rounded ${mode === key ? 'bg-win shadow-sm font-medium' : 'text-dim'}`}
              >
                {label}
              </button>
            ))}
          </div>
        )}
      </div>
      <label className="flex items-center gap-1.5 rounded-md px-2 h-7 mb-3 ring-1 ring-black/5 dark:ring-white/10 bg-win">
        <Search className="w-3.5 h-3.5 shrink-0 text-dim" aria-hidden />
        <input
          type="search"
          value={query}
          onChange={(e) => setQuery(e.target.value)}
          placeholder="Filter files"
          aria-label="Filter files"
          className="flex-1 min-w-0 bg-transparent outline-none text-[12px]"
        />
        <kbd className="text-[10px] text-dim">⌘P</kbd>
      </label>
      <div className="text-[13px] -mx-1">
        {error !== null && <div className="px-2 py-6 text-center text-[12px] text-dim">{error}</div>}
        {error === null && tree === null && <div className="px-2 py-6 text-center text-[12px] text-dim">Loading…</div>}
        {tree !== null && rows.length === 0 && <div className="px-2 py-6 text-center text-[12px] text-dim">No files match</div>}
        {rows.map((row) =>
          row.node.dir ? (
            <FolderRow key={row.node.path} row={row} onToggle={toggle} />
          ) : (
            <FileRow key={row.node.path} row={row} selected={selected === row.node.path} onOpen={openFile} />
          ),
        )}
      </div>
      {tree && (
        <div className="mt-3 pt-2 border-t border-line text-[11px] flex text-dim">
          <span className="flex-1">{`${files} files${tree.truncated ? '+' : ''}${isGit && changed ? ` · ${changed} changed` : ''}`}</span>
          {isGit && changed > 0 && (
            <button className="text-blue" onClick={() => emitUiCommand('openReview', sessionId)}>
              Review changes
            </button>
          )}
        </div>
      )}
    </div>
  );
}

const indent = (depth: number) => ({ paddingLeft: 6 + depth * 14 });

function FolderRow({ row, onToggle }: { row: Row; onToggle: (path: string) => void }) {
  const { node, depth, open, hasChanges } = row;
  const Icon = open ? FolderOpen : Folder;
  return (
    <button className={`${ROW} pr-2 text-left`} style={indent(depth)} aria-expanded={open} onClick={() => onToggle(node.path)}>
      <svg
        className="w-3 h-3 shrink-0 transition-transform opacity-50"
        style={{ transform: `rotate(${open ? 90 : 0}deg)` }}
        viewBox="0 0 12 12"
        fill="none"
        stroke="currentColor"
        strokeWidth="1.6"
        aria-hidden
      >
        <path d="M4.5 2.5 8 6l-3.5 3.5" />
      </svg>
      <Icon className="w-4 h-4 shrink-0 text-blue" aria-hidden />
      <span className="truncate flex-1">{node.name}</span>
      {hasChanges && !open && <span className="w-1.5 h-1.5 rounded-full bg-orange" title="Contains changes" />}
    </button>
  );
}

function FileRow({ row, selected, onOpen }: { row: Row; selected: boolean; onOpen: (path: string) => void }) {
  const { node, depth } = row;
  const entry = node.entry;
  const status = entry?.status ?? null;
  const badge = status ? BADGE[status] : null;
  const copyPath = () => void navigator.clipboard?.writeText(node.path).catch(() => {});
  return (
    <div
      role="button"
      tabIndex={0}
      data-file={node.path}
      title={node.path}
      className={`group ${ROW} pr-1.5 cursor-default ${selected ? 'bg-sel' : ''}`}
      style={indent(depth)}
      onClick={() => onOpen(node.path)}
      onKeyDown={(e) => {
        if (e.target === e.currentTarget && (e.key === 'Enter' || e.key === ' ')) {
          e.preventDefault();
          onOpen(node.path);
        }
      }}
    >
      <span className="w-3 shrink-0" />
      <FileGlyph name={node.name} />
      <span className={`truncate flex-1 ${status ? 'font-medium' : ''}`}>{node.name}</span>
      <span className="flex items-center gap-1.5 group-hover:hidden group-focus-within:hidden text-[11px] tabular-nums">
        {entry && entry.added > 0 && <span className="text-green">{`+${entry.added}`}</span>}
        {entry && entry.removed > 0 && <span className="text-red">{`−${entry.removed}`}</span>}
        {badge && (
          <span data-badge className={`w-4 text-center text-[10px] font-semibold ${badge.tone}`} title={badge.label}>
            {badge.letter}
          </span>
        )}
      </span>
      <span className="hidden group-hover:flex group-focus-within:flex items-center gap-0.5">
        {/* TODO(beta): wire to the host's open-in-default-editor once the bridge has it (Request to the orchestrator) */}
        <button className={ICON_BUTTON} title="Open in external editor" aria-label="Open in external editor" disabled>
          <SquareArrowOutUpRight className="w-3.5 h-3.5" aria-hidden />
        </button>
        <button
          className={ICON_BUTTON}
          title="Copy path"
          aria-label="Copy path"
          onClick={(e) => {
            e.stopPropagation();
            copyPath();
          }}
        >
          <Copy className="w-3.5 h-3.5" aria-hidden />
        </button>
      </span>
    </div>
  );
}
