import type { SettingsDraft } from './useSettings';

/**
 * "Agents may start sessions without asking" (off by default): an agent's `start_session` then skips the approval
 * card when the child stays in the agent's own project. Off, every start shows its full prompt first.
 */
export function ChildSessionsSetting({ draft }: { draft: SettingsDraft }) {
  const { settings, update } = draft;
  if (!settings) return null;
  return (
    <section aria-labelledby="sec-child-sessions" className="mt-8">
      <h3 id="sec-child-sessions" className="mb-2 text-[15px] font-semibold">Child sessions</h3>
      <label className="flex items-center gap-2">
        <input
          type="checkbox"
          className="accent-blue"
          checked={settings.agents_start_sessions_without_asking}
          onChange={(e) => update((s) => { s.agents_start_sessions_without_asking = e.target.checked; })}
        />
        Agents may start sessions without asking
      </label>
      <p className="mt-1 text-[12px] text-dim">
        Off: every session an agent asks for shows its full prompt on an approval card first. On: only starts inside the agent&apos;s own
        project skip the card; other projects still ask. Limits (two levels deep, five children per session) always apply.
      </p>
    </section>
  );
}
