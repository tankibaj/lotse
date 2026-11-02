import { useEffect, useRef, useState } from 'react';
import { useClient } from '../lib/client';
import { onUiCommand } from '../lib/ui/commands';
import { openInSplit } from '../panes/splitBus';
import { selectSession, getSelectedSessionId } from '../lib/ui/selection';
import { openOverlay, openSettings, closeOverlay, toggleOverlay, useOverlay } from '../sheets/overlay';
import { Modal } from '../sheets/ui';
import { useSnapshot } from '../sheets/useSnapshot';
import { StatusGlyph } from './StatusGlyph';
import { nextNeedingYou, paletteItems } from './items';
import type { PaletteItem } from './items';

function PaletteView() {
  const client = useClient();
  const snap = useSnapshot();
  const [query, setQuery] = useState('');
  const [sel, setSel] = useState(0);
  const list = useRef<HTMLDivElement>(null);

  const items = snap ? paletteItems(query, snap.projects, snap.sessions, snap.settings) : [];
  const active = Math.min(sel, Math.max(0, items.length - 1));

  useEffect(() => {
    list.current?.querySelector('[aria-selected="true"]')?.scrollIntoView?.({ block: 'nearest' });
  }, [active, items.length]);

  const run = (item: PaletteItem) => {
    if (!snap) return;
    closeOverlay();
    const a = item.action;
    if (a.type === 'session') selectSession(a.sessionId);
    else if (a.type === 'project') {
      const latest = snap.sessions.filter((s) => s.project_id === a.projectId).sort((x, y) => y.last_activity - x.last_activity)[0];
      if (latest) selectSession(latest.id);
      else openOverlay({ name: 'newSession', projectId: a.projectId });
    } else {
      switch (a.id) {
        case 'next-need': {
          const next = nextNeedingYou(snap.sessions, getSelectedSessionId());
          if (next) selectSession(next.id);
          break;
        }
        case 'add-project': openOverlay({ name: 'addProject' }); break;
        case 'new-session': openOverlay({ name: 'newSession' }); break;
        case 'split-right': openInSplit(null, 'right'); break;
        case 'split-down': openInSplit(null, 'down'); break;
        case 'settings': openSettings(); break;
        case 'shortcuts': openOverlay({ name: 'shortcuts' }); break;
        case 'pause': {
          const next = structuredClone(snap.settings);
          next.notifications.paused = !next.notifications.paused;
          void client.updateSettings(next).catch(() => {});
          break;
        }
      }
    }
  };

  const onKey = (e: React.KeyboardEvent) => {
    if (e.key === 'ArrowDown') {
      e.preventDefault();
      setSel(Math.min(active + 1, items.length - 1));
    } else if (e.key === 'ArrowUp') {
      e.preventDefault();
      setSel(Math.max(active - 1, 0));
    } else if (e.key === 'Enter' && items[active]) {
      e.preventDefault();
      run(items[active]);
    }
  };

  return (
    <Modal label="Command palette" onClose={closeOverlay} panelClass="mt-24 w-[560px] overflow-hidden rounded-xl">
      <input
        data-autofocus
        role="combobox"
        aria-expanded="true"
        aria-controls="palette-list"
        aria-activedescendant={items[active] ? `pal-${items[active].key}` : undefined}
        aria-label="Command palette"
        className="w-full border-b bg-transparent px-4 py-3 text-[15px] outline-none"
        placeholder="Jump to session, project or command…"
        value={query}
        autoComplete="off"
        spellCheck={false}
        onChange={(e) => { setQuery(e.target.value); setSel(0); }}
        onKeyDown={onKey}
      />
      <div id="palette-list" ref={list} role="listbox" aria-label="Results" className="max-h-[360px] overflow-y-auto py-1">
        {items.map((i, k) => (
          <div
            key={i.key}
            id={`pal-${i.key}`}
            role="option"
            aria-selected={k === active}
            className={`flex cursor-pointer items-center gap-3 px-4 py-2 ${k === active ? 'bg-sel' : ''}`}
            onMouseMove={() => k !== active && setSel(k)}
            onClick={() => run(i)}
          >
            <span className="flex w-4 justify-center" aria-hidden={!i.status}>{i.status ? <StatusGlyph status={i.status} /> : i.glyph}</span>
            <span className="flex-1 truncate">{i.label}</span>
            <span className="font-mono text-[11px] text-dim">{i.hint}</span>
          </div>
        ))}
        {snap && items.length === 0 && <div className="px-4 py-3 text-dim">No matches</div>}
      </div>
    </Modal>
  );
}

/** ⌘K command palette: jump to a session or project, run a command (mock `#pal`). Listens for `openPalette`. */
export function Palette() {
  const overlay = useOverlay();

  useEffect(() => {
    const off = onUiCommand('openPalette', () => openOverlay({ name: 'palette' }));
    const onKey = (e: KeyboardEvent) => {
      if (e.metaKey && !e.ctrlKey && !e.altKey && !e.shiftKey && e.code === 'KeyK') {
        e.preventDefault();
        toggleOverlay({ name: 'palette' });
      }
    };
    addEventListener('keydown', onKey);
    return () => {
      off();
      removeEventListener('keydown', onKey);
    };
  }, []);

  return overlay?.name === 'palette' ? <PaletteView /> : null;
}
