import { useCallback, useEffect, useState } from 'react';
import { useClient } from '../lib/client';
import type { DaemonEvent } from '../lib/client/types';
import { AGENT_LABEL, useWorkspace } from '../lib/state';
import { btnCls, Modal, primaryCls } from './ui';

type Request = Extract<DaemonEvent, { event: 'session_start_requested' }>['data'];

/**
 * Approval card for an agent that asks to start a child session (`session_start_requested`). It shows the
 * FULL prompt: a prompt injection can drive the request, so the user sees what would be typed. Requests queue,
 * oldest first. Esc / Dismiss decline; the daemon withdrawing a request (agent gone, parent closed) removes its card.
 */
export function StartRequestCards() {
  const client = useClient();
  const { sessions, projects } = useWorkspace();
  const [open, setOpen] = useState<Request[]>([]);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  useEffect(
    () =>
      client.onEvent((e) => {
        if (e.event === 'session_start_requested') setOpen((l) => (l.some((r) => r.id === e.data.id) ? l : [...l, e.data]));
        else if (e.event === 'session_start_withdrawn') setOpen((l) => l.filter((r) => r.id !== e.data.id));
      }),
    [client],
  );

  const current = open[0];
  const answer = useCallback(
    async (approve: boolean) => {
      if (!current || busy) return;
      setBusy(true);
      setError(null);
      try {
        await client.resolveSessionStart(current.id, approve);
        setOpen((l) => l.filter((r) => r.id !== current.id));
      } catch (e) {
        // already answered or withdrawn: the card is stale. Anything else (the start failed): keep it, say why.
        const gone = typeof e === 'object' && e !== null && (e as { code?: string }).code === 'not_found' && !approve;
        if (gone) setOpen((l) => l.filter((r) => r.id !== current.id));
        else setError(e instanceof Error ? e.message : 'Could not start the session');
      } finally {
        setBusy(false);
      }
    },
    [client, current, busy],
  );
  const decline = useCallback(() => void answer(false), [answer]);

  if (!current) return null;
  const parent = sessions.find((s) => s.id === current.parent_session);
  const project = projects.find((p) => p.id === current.project_id);
  return (
    <Modal
      key={current.id}
      label="Start a session?"
      onClose={decline}
      zClass="z-50"
      panelClass="relative mt-20 mb-8 w-[560px] rounded-xl p-5 text-[13px]"
    >
      <h3 className="mb-1 font-semibold">
        {parent?.title || 'An agent'} wants to start a session
        {open.length > 1 && <span className="ml-2 text-[11px] font-normal text-dim">1 of {open.length}</span>}
      </h3>
      <p className="mb-3 text-[12px] text-dim">
        {AGENT_LABEL[current.agent]} · {project?.name ?? current.project_id}
        {current.worktree && <> · new worktree <span className="font-mono">{current.worktree.branch}</span></>}
        {current.title && <> · “{current.title}”</>}
      </p>
      <div className="mb-1 text-[12px] text-dim">It will be typed into the new session:</div>
      <pre
        data-part="prompt"
        className="mb-3 max-h-[320px] overflow-auto rounded-md bg-side p-2 font-mono text-[12px] whitespace-pre-wrap break-words ring-1 ring-line"
        tabIndex={0}
      >
        {current.prompt}
      </pre>
      {error && (
        <div role="alert" className="mb-3 text-[11px] text-red">
          {error}
        </div>
      )}
      <div className="flex justify-end gap-2">
        <button type="button" className={btnCls} disabled={busy} onClick={decline}>
          Dismiss
        </button>
        <button type="button" data-autofocus className={primaryCls} disabled={busy} onClick={() => void answer(true)}>
          Start session
        </button>
      </div>
    </Modal>
  );
}
