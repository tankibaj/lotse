import { useEffect } from 'react';
import { useClient } from '../lib/client';
import { onUiCommand } from '../lib/ui/commands';
import { AddProjectSheet } from './AddProjectSheet';
import { hostBridge } from './hostBridge';
import { NewSessionSheet } from './NewSessionSheet';
import { openOverlay, openProjectStylePicker, toggleOverlay, useOverlay } from './overlay';
import { ProjectStylePicker } from './ProjectStylePicker';
import { ShortcutsSheet } from './ShortcutsSheet';

/** New session, Add project, project colour/icon popover and the shortcuts sheet. Listens for `openNewSession` / `openAddProject`. */
export function Sheets() {
  const client = useClient();
  const overlay = useOverlay();

  useEffect(() => {
    const offs = [
      onUiCommand('openNewSession', (projectId) => openOverlay({ name: 'newSession', projectId })),
      onUiCommand('openAddProject', () => openOverlay({ name: 'addProject' })),
      onUiCommand('openProjectStyle', (projectId) => {
        if (!projectId) return;
        // below the project's tile, like the mock's popover
        const row = document.querySelector<HTMLElement>(`[data-pid="${CSS.escape(projectId)}"]`);
        const r = row?.getBoundingClientRect();
        openProjectStylePicker(projectId, r ? { left: r.left + 8, bottom: r.top + 28 } : undefined);
      }),
    ];
    const onKey = (e: KeyboardEvent) => {
      if (!e.metaKey || e.ctrlKey) return;
      // ⌘T (new terminal) and ⌥⌘T (new Claude Code session) start sessions directly: see `useSessionShortcuts`.
      if (e.code === 'KeyO' && e.shiftKey && !e.altKey) {
        e.preventDefault();
        openOverlay({ name: 'addProject' });
      } else if (!e.altKey && ((e.code === 'Slash' && !e.shiftKey) || e.key === '/')) {
        // `key` too: on a German layout "/" is ⇧7
        e.preventDefault();
        toggleOverlay({ name: 'shortcuts' });
      }
    };
    addEventListener('keydown', onKey);
    // Dev server only: the picker is opened from the sidebar menu (WP3), so e2e needs a way in.
    if (import.meta.env.DEV) {
      const w = window as unknown as { __lotse?: object };
      w.__lotse = { ...w.__lotse, openProjectStylePicker };
    }
    return () => {
      offs.forEach((off) => off());
      removeEventListener('keydown', onKey);
    };
  }, []);

  // Drag a folder from Finder onto the sidebar: add it, or offer each repo of a multi-repo folder.
  useEffect(() => {
    const side = () => document.querySelector<HTMLElement>('[data-slot="sidebar"]');
    const bridge = hostBridge();
    const addDropped = async (paths: string[]) => {
      try {
        const [only] = paths;
        if (paths.length === 1 && only) {
          const found = await bridge.scanRepos(only);
          if (!found.is_repo && found.repos.length > 1) {
            openOverlay({ name: 'addProject', repos: { root: only, repos: found.repos } });
            return;
          }
        }
        for (const path of paths) await client.addProject({ path, name: null, color: null, icon: null });
      } catch {
        openOverlay({ name: 'addProject' }); // let the sheet explain what went wrong
      }
    };
    return bridge.watchDrops(side, {
      onHover: (over) => {
        const el = side();
        if (el) el.style.outline = over ? '2px dashed var(--blue)' : '';
      },
      onDrop: (paths) => paths.length && void addDropped(paths),
    });
  }, [client]);

  return (
    <>
      {overlay?.name === 'newSession' && <NewSessionSheet key={overlay.projectId ?? ''} />}
      {overlay?.name === 'addProject' && <AddProjectSheet key={overlay.repos?.root ?? 'home'} />}
      {overlay?.name === 'shortcuts' && <ShortcutsSheet />}
      {overlay?.name === 'projectStyle' && <ProjectStylePicker key={overlay.projectId} />}
    </>
  );
}
