import { useCallback, useEffect, useState } from 'react';
import { FileCode, FileJson, FileText, Maximize2, Minimize2, X } from 'lucide-react';
import { Editor } from '../editor';
import type { EditorChange } from '../editor';
import { useClient } from '../lib/client';
import type { Session } from '../lib/client';
import { emitUiCommand } from '../lib/ui/commands';

const AGENT_NAME: Partial<Record<Session['agent'], string>> = { claude_code: 'claude', codex: 'codex', opencode: 'opencode' };

function FileIcon({ name }: { name: string }) {
  const props = { size: 14, className: 'shrink-0 text-dim', 'aria-hidden': true } as const;
  if (/\.(tsx?|jsx?|rs|py|go|css|html)$/.test(name)) return <FileCode {...props} />;
  return /\.json$/.test(name) ? <FileJson {...props} /> : <FileText {...props} />;
}

function ExpandIcon({ expanded }: { expanded: boolean }) {
  return expanded ? <Minimize2 size={14} aria-hidden="true" /> : <Maximize2 size={14} aria-hidden="true" />;
}

/** What the session's agent changed in `path` (lines added/removed against the diff base), kept current while the session works. */
function useAgentChange(session: Session, path: string): EditorChange | undefined {
  const client = useClient();
  const [change, setChange] = useState<EditorChange | undefined>();
  const agent = AGENT_NAME[session.agent];
  useEffect(() => {
    if (agent === undefined) return;
    let live = true;
    const load = () =>
      client.getDiff(session.id).then(
        (d) => {
          const f = d.files.find((x) => x.path === path);
          if (live) setChange(f ? { added: f.added ?? 0, removed: f.removed ?? 0, by: `${agent} in “${session.title}”` } : undefined);
        },
        () => undefined,
      );
    void load();
    const off = client.onEvent((e) => {
      if ((e.event === 'status_changed' && e.data.session_id === session.id) || (e.event === 'session_updated' && e.data.id === session.id)) void load();
    });
    return () => {
      live = false;
      off();
    };
  }, [client, session.id, session.title, agent, path]);
  return change;
}

interface Props {
  session: Session;
  path: string;
  projectName: string;
  focused: boolean;
  expanded: boolean;
  /** Reports unsaved edits under the pane's key. */
  onDirty: (key: string, dirty: boolean) => void;
  paneKey: string;
  dirty: boolean;
  onToggleExpand: () => void;
  onClose: () => void;
}

/** The editor pane: the header of spec/mock.html (icon, name, unsaved dot, project/path, expand, close) over the editor. */
export function EditorPane({ session, path, projectName, focused, expanded, onDirty, paneKey, dirty, onToggleExpand, onClose }: Props) {
  const change = useAgentChange(session, path);
  const name = path.split('/').pop() ?? path;
  const dir = path.split('/').slice(0, -1).join('/');
  const reportDirty = useCallback((d: boolean) => onDirty(paneKey, d), [onDirty, paneKey]);
  const review = useCallback(() => void emitUiCommand('openReview', session.id, path), [session.id, path]);
  const stop = { onMouseDown: (e: { stopPropagation: () => void }) => e.stopPropagation() };
  return (
    <>
      <div
        className="flex h-7 shrink-0 items-center gap-1.5 overflow-hidden border-b border-line pr-1.5 pl-3 text-[12px] whitespace-nowrap"
        style={focused ? { boxShadow: 'inset 0 2px 0 var(--blue)' } : undefined}
      >
        <FileIcon name={name} />
        <span className="shrink-0 font-medium">{name}</span>
        {dirty && <span className="size-1.5 shrink-0 rounded-full bg-txt" title="Unsaved changes" />}
        <span className="min-w-0 truncate text-[11px] text-dim" title={`${projectName}/${path}`}>
          {dir ? `${projectName}/${dir}` : projectName}
        </span>
        <span className="flex-1" />
        <button
          className="grid size-5 shrink-0 place-items-center rounded-md text-dim hover:bg-sel"
          aria-label={expanded ? 'Restore editor' : 'Expand editor'}
          title={`${expanded ? 'Restore' : 'Expand'} ⌥⌘↩`}
          {...stop}
          onClick={(e) => {
            e.stopPropagation();
            onToggleExpand();
          }}
        >
          <ExpandIcon expanded={expanded} />
        </button>
        <button
          className="grid size-5 shrink-0 place-items-center rounded-md text-dim hover:bg-sel"
          aria-label="Close editor"
          title="Close ⌘W"
          {...stop}
          onClick={(e) => {
            e.stopPropagation();
            onClose();
          }}
        >
          <X size={12} aria-hidden="true" />
        </button>
      </div>
      <div className="min-h-0 flex-1">
        <Editor sessionId={session.id} path={path} change={change} onReview={change ? review : undefined} onDirtyChange={reportDirty} />
      </div>
    </>
  );
}
