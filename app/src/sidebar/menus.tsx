import type { DaemonClient } from '../lib/client/DaemonClient';
import type { NotificationMode, Project, Session, Settings } from '../lib/client/types';
import type { MenuEntry } from './ContextMenu';

type CustomMode = Extract<NotificationMode, { mode: 'custom' }>;
type EventKey = 'perm' | 'fail' | 'done' | 'sub_agent_done';

const EVENTS: { key: EventKey; label: string; glyph: string; tone: string }[] = [
  { key: 'perm', label: 'Needs permission', glyph: '▲', tone: 'text-orange' },
  { key: 'fail', label: 'Failed', glyph: '✕', tone: 'text-red' },
  { key: 'done', label: 'Done', glyph: '✓', tone: 'text-green' },
  { key: 'sub_agent_done', label: 'Sub-agent done', glyph: '✓', tone: 'text-green' },
];

/** What "custom" starts from: the global banner toggles (Settings › Notifications). */
const fromGlobal = (s: Settings | null): CustomMode => ({
  mode: 'custom',
  perm: s?.notifications.perm.banner ?? true,
  fail: s?.notifications.fail.banner ?? true,
  done: s?.notifications.done.banner ?? true,
  sub_agent_done: s?.notifications.sub_agent_done.banner ?? false,
});

export interface ProjectMenuActions {
  newSession: () => void;
  rename: () => void;
  remove: () => void;
}

/** Project context menu (spec/research.html#sidebar): New session, Notifications, icon/colour, Finder, path, Rename, Remove. */
export function projectMenu(
  project: Project,
  settings: Settings | null,
  client: DaemonClient,
  actions: ProjectMenuActions,
): MenuEntry[] {
  const mode = project.notifications;
  const set = (notifications: NotificationMode) => () => void client.updateProject(project.id, { notifications }).catch(() => {});
  const custom = mode.mode === 'custom' ? mode : fromGlobal(settings);
  const eventItems: MenuEntry[] = EVENTS.map(({ key, label, glyph, tone }) => ({
    kind: 'item',
    label: (
      <>
        <span className={tone} aria-hidden="true">
          {glyph}
        </span>{' '}
        {label}
      </>
    ),
    checked: mode.mode === 'custom' ? mode[key] : fromGlobal(settings)[key],
    disabled: mode.mode !== 'custom',
    keepOpen: true,
    onSelect: set({ ...custom, [key]: !custom[key] }),
  }));
  return [
    { kind: 'header', label: project.name },
    { kind: 'item', label: 'New Session…', hint: '⌘T', onSelect: actions.newSession },
    { kind: 'separator' },
    {
      kind: 'item',
      label: 'Notifications',
      submenu: [
        { kind: 'item', label: 'Use global defaults', checked: mode.mode === 'global', keepOpen: true, onSelect: set({ mode: 'global' }) },
        { kind: 'item', label: 'Custom for this project', checked: mode.mode === 'custom', keepOpen: true, onSelect: mode.mode === 'custom' ? undefined : set(fromGlobal(settings)) },
        ...eventItems,
        { kind: 'separator' },
        { kind: 'item', label: 'Mute this project', checked: mode.mode === 'mute', keepOpen: true, onSelect: set({ mode: 'mute' }) },
        { kind: 'separator' },
        { kind: 'note', label: 'Global defaults: Settings › Notifications' },
      ],
    },
    // Unavailable until the icon picker (sheets) and the native bridge expose a command for them.
    { kind: 'item', label: 'Change Icon & Color…', disabled: true, hint: 'Soon' },
    { kind: 'separator' },
    { kind: 'item', label: 'Reveal in Finder', disabled: true, hint: 'Soon' },
    {
      kind: 'item',
      label: 'Copy Path',
      onSelect: () => void navigator.clipboard?.writeText(project.path).catch(() => {}),
    },
    { kind: 'item', label: 'Rename…', onSelect: actions.rename },
    { kind: 'separator' },
    { kind: 'item', label: 'Remove from Lotse…', danger: true, onSelect: actions.remove },
  ];
}

export interface SessionMenuActions {
  open: () => void;
  rename: () => void;
  stop: () => void;
}

/** Session context menu: Open, Split right, Rename, Show transcript / changes (Beta), Stop. */
export function sessionMenu(session: Session, actions: SessionMenuActions): MenuEntry[] {
  return [
    { kind: 'header', label: session.title },
    { kind: 'item', label: 'Open', hint: '⏎', onSelect: actions.open },
    { kind: 'item', label: 'Split Right', disabled: true, hint: 'Soon' },
    { kind: 'separator' },
    { kind: 'item', label: 'Rename…', onSelect: actions.rename },
    { kind: 'item', label: 'Show Transcript', disabled: true, hint: 'Beta' },
    { kind: 'item', label: 'Show Changes', disabled: true, hint: 'Beta' },
    { kind: 'separator' },
    { kind: 'item', label: 'Stop', danger: true, onSelect: actions.stop },
  ];
}
