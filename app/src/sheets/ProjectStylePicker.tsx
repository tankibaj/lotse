import { useState } from 'react';
import { useClient } from '../lib/client';
import { closeOverlay, useOverlay } from './overlay';
import { SWATCHES, ProjectIcon, filterIcons } from './symbols';
import { Modal } from './ui';
import { useSnapshot } from './useSnapshot';

/**
 * Project colour and icon popover (mock `#icoPop`): 15 swatches, searchable symbol grid. Every pick is
 * saved at once through `updateProject` and stays open for the next pick.
 */
export function ProjectStylePicker() {
  const client = useClient();
  const overlay = useOverlay();
  const snap = useSnapshot();
  const [query, setQuery] = useState('');
  const [failure, setFailure] = useState<string | null>(null);
  if (overlay?.name !== 'projectStyle') return null;

  const project = snap?.projects.find((p) => p.id === overlay.projectId);
  const left = Math.max(8, Math.min(overlay.x, innerWidth - 350));
  const top = Math.max(8, Math.min(overlay.y, innerHeight - 460));

  const save = (patch: { color: string } | { icon: string }) => {
    setFailure(null);
    client
      .updateProject(overlay.projectId, patch)
      .catch((e: unknown) => setFailure(e instanceof Error ? e.message : 'Could not save'));
  };
  const groups = filterIcons(query);

  return (
    <Modal
      label="Project icon and color"
      onClose={closeOverlay}
      tone="bg-transparent"
      zClass="z-50"
      panelClass="absolute w-[340px] rounded-2xl overflow-hidden backdrop-blur-xl"
      panelStyle={{ left, top, background: 'color-mix(in srgb, var(--win) 92%, transparent)' }}
    >
      <div className="grid grid-cols-8 gap-2.5 p-4 pb-3" role="group" aria-label="Colour">
        {Object.entries(SWATCHES).map(([key, hex]) => {
          const on = project?.color === key;
          return (
            <button
              key={key}
              type="button"
              className="grid size-7 place-items-center rounded-full text-[13px] text-white"
              style={{ background: hex, boxShadow: on ? `0 0 0 2px var(--win), 0 0 0 4px ${hex}` : undefined }}
              aria-label={key}
              aria-pressed={on}
              onClick={() => save({ color: key })}
            >
              {on ? '✓' : ''}
            </button>
          );
        })}
      </div>
      <div className="border-b px-3 pb-3">
        <input
          data-autofocus
          className="w-full rounded-lg bg-side px-3 py-1.5 text-[13px] ring-1 ring-line outline-none focus:ring-2"
          placeholder="Search symbols"
          aria-label="Search symbols"
          value={query}
          onChange={(e) => setQuery(e.target.value)}
        />
      </div>
      {failure && <div role="alert" className="px-4 pt-2 text-[12px] text-red">{failure}</div>}
      <div className="max-h-[300px] overflow-y-auto pb-2">
        {groups.map(([group, names]) => (
          <div key={group}>
            <div className="px-4 pb-1 pt-3 text-[11px] font-semibold uppercase tracking-wide text-dim">{group}</div>
            <div className="grid grid-cols-8 gap-1 px-3">
              {names.map((n) => (
                <button
                  key={n}
                  type="button"
                  className={`grid h-9 place-items-center rounded-lg hover:bg-black/5 dark:hover:bg-white/10 ${project?.icon === n ? 'bg-sel text-blue' : ''}`}
                  aria-label={n}
                  aria-pressed={project?.icon === n}
                  title={n}
                  onClick={() => save({ icon: n })}
                >
                  <ProjectIcon name={n} />
                </button>
              ))}
            </div>
          </div>
        ))}
        {groups.length === 0 && <div className="px-4 py-6 text-center text-[13px] text-dim">No symbols</div>}
      </div>
    </Modal>
  );
}
