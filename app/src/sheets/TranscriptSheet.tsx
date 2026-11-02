import { useEffect, useRef } from 'react';
import { useClient } from '../lib/client';
import type { SessionId, TranscriptTurn } from '../lib/client/types';
import { AGENT_LABEL, useWorkspace } from '../lib/state';
import { closeOverlay } from './overlay';
import { Modal, btnCls } from './ui';
import { useLoad } from './useLoad';

// Show Transcript (session menu): the last turns of the agent's own transcript, read-only. Claude Code and
// Codex only (their hooks name the file); the daemon keeps the size small and leaves tool output out.

const time = (at: number | null) => (at === null ? '' : new Date(at).toLocaleTimeString([], { hour: '2-digit', minute: '2-digit' }));

function Turn({ turn, agent }: { turn: TranscriptTurn; agent: string }) {
  if (turn.role === 'tool' && turn.tool) {
    return (
      <div data-role="tool" className="flex items-baseline gap-2 pl-[84px] font-mono text-[12px] text-dim">
        <span className="shrink-0 font-semibold">{turn.tool.name}</span>
        <span className="min-w-0 truncate" title={turn.tool.summary}>
          {turn.tool.summary}
        </span>
      </div>
    );
  }
  const you = turn.role === 'user';
  return (
    <div data-role={turn.role} className="flex gap-3">
      <div className={`w-[72px] shrink-0 pt-px text-right text-[11px] font-semibold ${you ? 'text-blue' : 'text-dim'}`}>
        {you ? 'You' : agent}
        <div className="font-normal text-dim">{time(turn.at)}</div>
      </div>
      <div className="min-w-0 flex-1 whitespace-pre-wrap break-words">{turn.text}</div>
    </div>
  );
}

/** Show Transcript sheet for `sessionId`. */
export function TranscriptSheet({ sessionId }: { sessionId: SessionId }) {
  const client = useClient();
  const { sessions } = useWorkspace();
  const session = sessions.find((s) => s.id === sessionId);
  const agent = session ? AGENT_LABEL[session.agent] : 'Agent';
  const [state, reload] = useLoad(() => client.getTranscript(sessionId));
  const end = useRef<HTMLDivElement>(null);
  useEffect(() => {
    if (state.status === 'ready') end.current?.scrollIntoView?.({ block: 'end' }); // newest turn in view
  }, [state]);

  return (
    <Modal label="Transcript" onClose={closeOverlay} panelClass="relative mt-12 mb-8 w-[760px] rounded-xl text-[13px]">
      <div className="flex items-center gap-3 border-b px-4 py-3">
        <h3 className="font-semibold">Transcript</h3>
        <span className="min-w-0 flex-1 truncate text-dim" title={session?.title}>
          {session?.title ?? ''} · {agent}
        </span>
        <button type="button" className={btnCls} onClick={reload} disabled={state.status === 'loading'}>
          Refresh
        </button>
        <button type="button" data-autofocus onClick={closeOverlay} className="grid h-[26px] w-7 place-items-center rounded-md text-dim hover:bg-sel" aria-label="Close">
          ✕
        </button>
      </div>

      <div className="max-h-[68vh] overflow-y-auto px-4 py-3" aria-live="polite">
        {state.status === 'loading' && <div className="py-6 text-center text-dim">Loading transcript…</div>}
        {state.status === 'error' && (
          <div role="alert" className="py-6 text-center text-red">
            {state.message}
          </div>
        )}
        {state.status === 'ready' && !state.data.available && (
          <div className="py-6 text-center text-dim">No transcript yet. {agent} reports it with its first hook event, once a prompt has run.</div>
        )}
        {state.status === 'ready' && state.data.available && state.data.turns.length === 0 && <div className="py-6 text-center text-dim">The transcript has no turns yet.</div>}
        {state.status === 'ready' && state.data.available && state.data.turns.length > 0 && (
          <div className="space-y-2.5">
            {state.data.truncated && (
              <div role="status" className="pb-1 text-center text-[12px] text-dim">
                Older turns are not shown.
              </div>
            )}
            {state.data.turns.map((t, i) => (
              <Turn key={i} turn={t} agent={agent} />
            ))}
            <div ref={end} />
          </div>
        )}
      </div>
    </Modal>
  );
}
