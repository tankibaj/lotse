import type { KeyboardEvent } from 'react';
import { emitUiCommand } from '../lib/ui/commands';
import { selectSession, useSelectedSessionId } from '../lib/ui/selection';
import { ProjectTile } from './ProjectTile';
import { StatusGlyph } from './StatusGlyph';
import { useSessions } from './useSessions';

/** Tabs of the selected session's project (one tab per session, like the mock) and the "+" button. */
export function TabStrip() {
  const { sessions, projects } = useSessions();
  const selected = useSelectedSessionId();
  const current = sessions.find((s) => s.id === selected);
  const project = projects.find((p) => p.id === current?.project_id);
  const tabs = current ? sessions.filter((s) => s.project_id === current.project_id) : [];

  const onKeyDown = (e: KeyboardEvent<HTMLDivElement>) => {
    if (e.key !== 'ArrowLeft' && e.key !== 'ArrowRight') return;
    const at = tabs.findIndex((s) => s.id === selected);
    const next = tabs[(at + (e.key === 'ArrowRight' ? 1 : -1) + tabs.length) % tabs.length];
    if (!next) return;
    e.preventDefault();
    selectSession(next.id);
    e.currentTarget.querySelector<HTMLElement>(`[data-tab="${next.id}"]`)?.focus();
  };

  return (
    <>
      <div className="flex min-w-0 items-end gap-1" role="tablist" aria-label={project ? `Sessions in ${project.name}` : 'Sessions'} onKeyDown={onKeyDown}>
        {tabs.map((s) => {
          const active = s.id === selected;
          return (
            <button
              key={s.id}
              role="tab"
              aria-selected={active}
              tabIndex={active ? 0 : -1}
              data-tab={s.id}
              className={`flex h-8 min-w-0 items-center gap-2 rounded-t-md border border-b-0 px-3 text-[12px] ${
                active ? 'border-line bg-term' : 'border-transparent opacity-60 hover:opacity-100'
              }`}
              onClick={(e) => {
                // A click hands the keyboard to the terminal; arrow keys keep it on the tab (see TerminalView).
                // A new session's terminal takes focus itself once the tab is blurred; the open one is told to.
                selectSession(s.id);
                e.currentTarget.blur();
                if (active) document.querySelector<HTMLElement>('[data-pane][data-focused="true"] textarea')?.focus();
              }}
            >
              <StatusGlyph status={s.status} />
              <span className="max-w-[200px] truncate">{s.title}</span>
              {project && (
                <span className="inline-flex shrink-0 items-center gap-1 text-tdim">
                  <ProjectTile project={project} />
                  {project.name}
                </span>
              )}
            </button>
          );
        })}
      </div>
      <button
        className="mb-2 grid h-[26px] w-7 shrink-0 place-items-center rounded-md text-dim hover:bg-sel"
        onClick={() => emitUiCommand('openNewSession')}
        aria-label="New session"
        title="New session ⌘T"
      >
        +
      </button>
    </>
  );
}
