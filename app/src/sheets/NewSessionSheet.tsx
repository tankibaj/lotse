import { useState } from 'react';
import type { FormEvent } from 'react';
import { useClient } from '../lib/client';
import type { AgentKind } from '../lib/client/types';
import { selectSession, useSelectedSessionId } from '../lib/ui/selection';
import { closeOverlay, getOverlay, openSettings } from './overlay';
import { Field, Modal, btnCls, labelCls, primaryCls } from './ui';
import { useSnapshot } from './useSnapshot';
import { AGENTS, validateNewSession } from './validation';

export const DEFAULT_BRANCH = 'lotse/new-task';

/** New session: project, what to run, optional worktree and first prompt (mock `#newS`). */
export function NewSessionSheet() {
  const client = useClient();
  const snap = useSnapshot();
  const selected = useSelectedSessionId();
  // Back from Settings › Agents ("Change…"): the form comes back as it was left.
  const [opened] = useState(() => {
    const o = getOverlay();
    return o?.name === 'newSession' ? o : null;
  });
  const [projectId, setProjectId] = useState<string | null>(opened?.projectId ?? null);
  const [agent, setAgent] = useState<AgentKind>(opened?.draft?.agent ?? 'terminal');
  const [worktree, setWorktree] = useState(opened?.draft?.worktree ?? false);
  const [branch, setBranch] = useState(opened?.draft?.branch ?? DEFAULT_BRANCH);
  const [prompt, setPrompt] = useState(opened?.draft?.prompt ?? '');
  const [busy, setBusy] = useState(false);
  const [failure, setFailure] = useState<string | null>(null);

  const projects = snap?.projects ?? [];
  const current = snap?.sessions.find((s) => s.id === selected)?.project_id;
  const pid = projectId ?? current ?? projects[0]?.id ?? '';
  const project = projects.find((p) => p.id === pid);
  const isAgent = agent !== 'terminal';
  // A ticked worktree does not apply to a folder without git (the box is disabled there): ignore it.
  const wtOn = worktree && !!project?.is_git;
  const errors = validateNewSession({ projectId: pid, agent, worktree: wtOn, branch }, projects);
  const command = snap?.settings.launch_commands[agent] ?? '';
  const valid = !errors.project && !errors.branch && !!snap;

  const submit = async (e?: FormEvent) => {
    e?.preventDefault();
    if (!valid || busy) return;
    setBusy(true);
    setFailure(null);
    try {
      const created = await client.createSession({
        project_id: pid,
        agent,
        command: null, // the daemon applies the launch command from Settings
        title: null,
        worktree: wtOn ? { branch: branch.trim() } : null,
        first_prompt: isAgent && prompt.trim() ? prompt.trim() : null,
      });
      selectSession(created.id);
      if (getOverlay()?.name === 'newSession') closeOverlay(); // not something opened since
    } catch (err) {
      setFailure(err instanceof Error ? err.message : 'Could not start the session');
      setBusy(false);
    }
  };

  return (
    <Modal label="New session" onClose={closeOverlay} panelClass="mt-24 w-[440px] rounded-xl p-5">
      <form onSubmit={submit} onKeyDown={(e) => e.key === 'Enter' && e.metaKey && void submit()}>
        <h3 className="mb-3 font-semibold">New session</h3>

        <label htmlFor="ns-project" className={labelCls}>Project</label>
        <select
          id="ns-project"
          data-autofocus
          className="mb-3 w-full rounded-md bg-side px-2 py-1.5"
          value={pid}
          onChange={(e) => setProjectId(e.target.value)}
        >
          {projects.map((p) => (
            <option key={p.id} value={p.id}>{p.name}</option>
          ))}
        </select>
        <Field id="ns-project-err" error={errors.project} />

        <div className={labelCls} id="ns-run">Run</div>
        <div className="mb-3 grid grid-cols-4 gap-2" role="group" aria-labelledby="ns-run">
          {AGENTS.map((a) => (
            <button
              key={a.kind}
              type="button"
              className="rounded-md py-2 text-[12px] ring-1 ring-line"
              aria-pressed={agent === a.kind}
              onClick={() => setAgent(a.kind)}
            >
              {a.label}
            </button>
          ))}
        </div>
        <div className="-mt-1 mb-3 font-mono text-[11px] text-dim" data-testid="cmd-hint">
          Runs <span className="text-txt">{command}</span> ·{' '}
          <button type="button" className="text-blue" onClick={() => openSettings('Agents', { name: 'newSession', ...(pid ? { projectId: pid } : {}), draft: { agent, worktree, branch, prompt } })}>Change…</button>
        </div>

        <label className="mb-3 flex items-center gap-2 text-[12px]">
          <input
            type="checkbox"
            checked={wtOn}
            disabled={!!project && !project.is_git}
            onChange={(e) => setWorktree(e.target.checked)}
          />
          Create isolated git worktree <span className="text-dim">{project && !project.is_git ? '(No git)' : '(optional)'}</span>
        </label>
        {wtOn && (
          <>
            <input
              className="-mt-1 mb-3 w-full rounded-md bg-side px-2 py-1.5 font-mono text-[12px]"
              aria-label="Worktree branch name"
              aria-invalid={!!errors.branch}
              aria-describedby={errors.branch ? 'ns-branch-err' : undefined}
              value={branch}
              spellCheck={false}
              onChange={(e) => setBranch(e.target.value)}
            />
            <Field id="ns-branch-err" error={errors.branch} />
          </>
        )}

        {isAgent && (
          <>
            <label htmlFor="ns-prompt" className={labelCls}>First prompt (optional)</label>
            <textarea
              id="ns-prompt"
              className="w-full rounded-md bg-side px-2 py-1.5 text-[12px]"
              rows={2}
              placeholder="What should the agent do?"
              value={prompt}
              onChange={(e) => setPrompt(e.target.value)}
            />
          </>
        )}

        {failure && <div role="alert" className="mt-3 text-[12px] text-red">{failure}</div>}
        <div className="mt-4 flex justify-end gap-2">
          <button type="button" className={btnCls} onClick={closeOverlay}>Cancel</button>
          <button type="submit" className={primaryCls} disabled={!valid || busy}>Start ⏎</button>
        </div>
      </form>
    </Modal>
  );
}
