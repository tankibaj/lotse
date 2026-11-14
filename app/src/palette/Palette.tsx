import { useEffect, useRef, useState } from 'react';
import { useClient } from '../lib/client';
import { emitUiCommand, onUiCommand } from '../lib/ui/commands';
import { openInSplit } from '../panes/splitBus';
import { startSession } from '../lib/state/actions';
import { getOpenPanel, togglePanel } from '../lib/ui/panel';
import type { PanelName } from '../lib/ui/panel';
import { selectSession, getSelectedSessionId } from '../lib/ui/selection';
import { openOverlay, openSettings, closeOverlay, toggleOverlay, useOverlay } from '../sheets/overlay';
import { Modal } from '../sheets/ui';
import { useSnapshot } from '../sheets/useSnapshot';
import { StatusGlyph } from './StatusGlyph';
import { nextNeedingYou, paletteItems } from './items';
import type { PaletteItem } from './items';

/** Open `name` and leave it open (`togglePanel` would close it when it is already the open one). */
const openPanel = (name: PanelName) => getOpenPanel() !== name && togglePanel(name);

const showPanel = (sessionId: string, name: PanelName) => {
  selectSession(sessionId);
  openPanel(name);
};

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
    const selected = getSelectedSessionId();
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
        case 'new-terminal': void startSession(client, snap, 'terminal').catch(() => {}); break;
        case 'new-claude': void startSession(client, snap, 'claude_code').catch(() => {}); break;
        case 'focus-prev': emitUiCommand('focusPane', 'prev'); break;
        case 'focus-next': emitUiCommand('focusPane', 'next'); break;
        case 'close-pane': emitUiCommand('closePane'); break;
        case 'toggle-sidebar': emitUiCommand('toggleSidebar'); break;
        case 'project-style': {
          const projectId = snap.sessions.find((s) => s.id === selected)?.project_id;
          if (projectId) emitUiCommand('openProjectStyle', projectId);
          break;
        }
        case 'export-transcript': openPanel('transcript'); break; // the Export menu is in that panel
        case 'split-right': openInSplit(null, 'right'); break;
        case 'split-down': openInSplit(null, 'down'); break;
        case 'panel-session': emitUiCommand('togglePanel', 'session'); break;
        case 'panel-changes': emitUiCommand('togglePanel', 'changes'); break;
        case 'panel-files': emitUiCommand('togglePanel', 'files'); break;
        case 'panel-transcript': emitUiCommand('togglePanel', 'transcript'); break;
        case 'panel-usage': emitUiCommand('togglePanel', 'usage'); break;
        case 'open-composer': emitUiCommand('openComposer'); break;
        // these act on the selected session: with none selected they do nothing
        case 'open-review': if (selected) emitUiCommand('openReview', selected); break;
        // the panels, directly: `showTranscript` / `showChanges` are still answered by the read-only sheets (Sheets.tsx)
        case 'show-transcript': if (selected) openPanel('transcript'); break;
        case 'show-changes': if (selected) openPanel('changes'); break;
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

  // Project prefixes line up: pad them to the longest name.
  const projectWidth = Math.max(0, ...items.map((i) => i.project?.length ?? 0));

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
            <span className="flex-1 truncate">
              {i.project !== undefined && (
                <>
                  <span className="inline-block text-dim" style={{ minWidth: `${projectWidth + 2}ch` }}>{i.project} ›</span>{' '}
                </>
              )}
              {i.label}
            </span>
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
    // Show Changes / Show Transcript (session menu, palette): the Beta panels, for that session.
    const offPanels = [
      onUiCommand('showChanges', (id) => id && showPanel(id, 'changes')),
      onUiCommand('showTranscript', (id) => id && showPanel(id, 'transcript')),
    ];
    const onKey = (e: KeyboardEvent) => {
      if (e.metaKey && !e.ctrlKey && !e.altKey && !e.shiftKey && e.code === 'KeyK') {
        e.preventDefault();
        toggleOverlay({ name: 'palette' });
      }
    };
    addEventListener('keydown', onKey);
    return () => {
      off();
      offPanels.forEach((o) => o());
      removeEventListener('keydown', onKey);
    };
  }, []);

  return overlay?.name === 'palette' ? <PaletteView /> : null;
}
