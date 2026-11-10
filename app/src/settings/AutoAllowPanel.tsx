import { useEffect, useRef, useState } from 'react';
import { useClient } from '../lib/client';
import type { Project } from '../lib/client/types';
import { PATH_TOOLS, RULE_TOOLS, autoAllowBridge, isPreviewBridge, validateConfig, validateRule } from './autoallow';
import type { AutoAllowConfig, AutoAllowRule } from './autoallow';

const input = 'rounded-md bg-side px-2 py-1 ring-1 ring-line';

const nextId = (rules: AutoAllowRule[]) => {
  let n = rules.length + 1;
  while (rules.some((r) => r.id === `r${n}`)) n++;
  return `r${n}`;
};

const describeRule = (r: AutoAllowRule) =>
  r.tool === 'Bash' ? `${r.command}${r.prefix ? ' …' : ''}` : `${r.tool} in ${r.dir ? `${r.dir}/` : 'the whole project'}`;

/** Settings › Agents: the daemon's auto-allow rules (off by default). The daemon matches and logs; this edits. */
export function AutoAllowPanel() {
  const client = useClient();
  const bridge = autoAllowBridge();
  const [config, setConfig] = useState<AutoAllowConfig | null>(null);
  const [projects, setProjects] = useState<Project[]>([]);
  const [projectsLoaded, setProjectsLoaded] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [form, setForm] = useState({ project: '', tool: 'Bash', command: '', dir: '', prefix: false, destructive: false });
  const alive = useRef(true);

  useEffect(() => {
    alive.current = true;
    client.getSnapshot().then((s) => alive.current && (setProjects(s.projects), setProjectsLoaded(true)), () => alive.current && setProjectsLoaded(true));
    bridge?.load().then((c) => alive.current && setConfig(c), (e) => alive.current && setError(message(e)));
    return () => {
      alive.current = false;
    };
    // the bridge is a module-level choice, fixed for the life of the page
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [client]);

  /** Save `next`; on a refusal the rules in force (`config`) stay and the reason shows. */
  const commit = async (next: AutoAllowConfig): Promise<boolean> => {
    const why = validateConfig(next);
    if (why || !bridge) {
      setError(why ?? 'not available yet');
      return false;
    }
    try {
      await bridge.save(next);
    } catch (e) {
      if (alive.current) setError(message(e));
      return false;
    }
    if (alive.current) {
      setConfig(next);
      setError(null);
    }
    return true;
  };

  const add = async () => {
    if (!config) return;
    const project = projects.find((p) => p.name === form.project) ?? projects[0];
    if (!project) return setError('add a project first');
    const bash = form.tool === 'Bash';
    const rule: AutoAllowRule = {
      id: nextId(config.rules),
      project_id: project.id,
      tool: form.tool,
      ...(bash ? { command: form.command, prefix: form.prefix } : { dir: form.dir.trim(), prefix: false }),
      allow_destructive: form.destructive,
    };
    const why = validateRule(rule);
    if (why) return setError(why);
    if (await commit({ ...config, rules: [...config.rules, rule] })) setForm((f) => ({ ...f, command: '', dir: '' }));
  };

  const disabled = !bridge || !config || !projectsLoaded;
  const projectName = (id: string) => projects.find((p) => p.id === id)?.name ?? id;
  const bash = form.tool === 'Bash';

  return (
    <section aria-labelledby="sec-autoallow" className="mt-8 border-t pt-5">
      <h3 id="sec-autoallow" className="mb-1 text-[15px] font-semibold">Auto-allow rules</h3>
      <p className="pb-2 text-[12px] text-dim">
        Off by default. When on, a tool call that matches a rule of its project skips the permission prompt; every one is logged to
        history. Commands must be plain (no <code>; &amp; | $</code>, quotes or <code>..</code>) and match exactly, or by leading words
        when the rule says so. Destructive commands and paths outside the project or in <code>.git</code>, <code>.env</code> and
        similar never match unless the rule says so.
      </p>
      {!bridge && <p className="pb-2 text-[12px] text-orange">Not available yet: the app cannot reach the daemon&apos;s rules.</p>}
      {bridge && isPreviewBridge() && <p className="pb-2 text-[12px] text-dim">Preview: nothing is saved in this browser build.</p>}
      {error && <div role="alert" className="pb-2 text-[12px] text-red">{error}</div>}
      <label className="flex items-center gap-2">
        <input
          type="checkbox"
          className="accent-blue"
          disabled={disabled}
          checked={config?.enabled ?? false}
          onChange={(e) => config && void commit({ ...config, enabled: e.target.checked })}
        />
        Auto-allow matching tool calls
      </label>
      <ul className="mt-3 text-[13px]" aria-label="Rules">
        {config?.rules.length === 0 && <li className="py-1 text-[12px] text-dim">No rules.</li>}
        {config?.rules.map((r) => (
          <li key={r.id} className="flex items-center gap-2 border-t py-1.5">
            <span className="text-[11px] text-dim">{projectName(r.project_id)}</span>
            <span className="text-[11px] text-dim">{r.tool}</span>
            <code className="min-w-0 flex-1 truncate font-mono text-[12px]">{describeRule(r)}</code>
            {r.allow_destructive && <span className="text-[11px] text-orange">destructive</span>}
            <button type="button" className="text-[12px] text-blue" aria-label={`Remove rule ${r.id}`} onClick={() => void commit({ ...config, rules: config.rules.filter((x) => x.id !== r.id) })}>
              Remove
            </button>
          </li>
        ))}
      </ul>
      {bridge && (
        <div className="mt-3 flex flex-wrap items-end gap-2 text-[12px]">
          <select className={input} aria-label="Rule project" value={form.project} onChange={(e) => setForm({ ...form, project: e.target.value })}>
            {projects.map((p) => <option key={p.id} value={p.name}>{p.name}</option>)}
          </select>
          <select className={input} aria-label="Rule tool" value={form.tool} onChange={(e) => setForm({ ...form, tool: e.target.value })}>
            {RULE_TOOLS.map((t) => <option key={t} value={t}>{t}</option>)}
          </select>
          {bash ? (
            <>
              <input className={`${input} min-w-[200px] flex-1 font-mono`} aria-label="Rule command" placeholder="git status" spellCheck={false} value={form.command} onChange={(e) => setForm({ ...form, command: e.target.value })} />
              <label className="flex items-center gap-1"><input type="checkbox" className="accent-blue" checked={form.prefix} onChange={(e) => setForm({ ...form, prefix: e.target.checked })} />Also with plain arguments</label>
            </>
          ) : (
            <input className={`${input} min-w-[200px] flex-1 font-mono`} aria-label="Rule directory" placeholder="src (empty = whole project)" spellCheck={false} value={form.dir} onChange={(e) => setForm({ ...form, dir: e.target.value })} />
          )}
          <label className="flex items-center gap-1 text-orange">
            <input type="checkbox" className="accent-blue" checked={form.destructive} onChange={(e) => setForm({ ...form, destructive: e.target.checked })} />
            Allow a destructive command or a secrets path
          </label>
          <button type="button" className="rounded-md bg-blue px-3 py-1 text-white disabled:opacity-50" disabled={disabled} onClick={() => void add()}>Add rule</button>
        </div>
      )}
      <p className="mt-2 text-[11px] text-dim">Path tools: {PATH_TOOLS.join(', ')}.</p>
    </section>
  );
}

const message = (e: unknown) => (typeof e === 'string' ? e : e instanceof Error ? e.message : 'failed');
