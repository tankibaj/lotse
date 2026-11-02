import type { DaemonClient } from '../lib/client/DaemonClient';
import type { AgentKind, NotificationMode, Project, Session, Settings, Workspace, WorkspaceId } from '../lib/client/types';
import { AGENT_LABEL } from '../lib/state/selectors';
import { emitUiCommand } from '../lib/ui/commands';
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
  changeStyle: () => void;
  workspaces: readonly Workspace[];
  /** Move the project into a workspace. */
  moveTo: (workspaceId: WorkspaceId) => void;
  /** "New Workspace…": ask for a name, create it and move the project there. */
  newWorkspace: () => void;
  /** Native only: `undefined` (browser) shows the item disabled. */
  reveal?: () => void;
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
    { kind: 'item', label: 'New Session…', onSelect: actions.newSession },
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
    { kind: 'item', label: 'Change Icon & Color…', onSelect: actions.changeStyle },
    {
      kind: 'item',
      label: 'Move to Workspace',
      submenu: [
        ...actions.workspaces.map(
          (w): MenuEntry => ({
            kind: 'item',
            label: w.name,
            checked: w.id === project.workspace_id,
            onSelect: w.id === project.workspace_id ? undefined : () => actions.moveTo(w.id),
          }),
        ),
        { kind: 'separator' },
        { kind: 'item', label: 'New Workspace…', onSelect: actions.newWorkspace },
      ],
    },
    { kind: 'separator' },
    actions.reveal
      ? { kind: 'item', label: 'Reveal in Finder', onSelect: actions.reveal }
      : { kind: 'item', label: 'Reveal in Finder', disabled: true, hint: 'App only' },
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
  split: () => void;
  splitDown: () => void;
  rename: () => void;
  stop: () => void;
}

export interface WorkspaceMenuActions {
  newWorkspace: () => void;
  rename: () => void;
  /** `undefined` for the default workspace, which stays. */
  remove?: () => void;
}

/** Workspace header context menu: New Workspace, Rename, Delete (not the default one). */
export function workspaceMenu(workspace: Workspace, actions: WorkspaceMenuActions): MenuEntry[] {
  return [
    { kind: 'header', label: workspace.name },
    { kind: 'item', label: 'New Workspace…', onSelect: actions.newWorkspace },
    { kind: 'item', label: 'Rename…', onSelect: actions.rename },
    { kind: 'separator' },
    actions.remove
      ? { kind: 'item', label: 'Delete Workspace…', danger: true, onSelect: actions.remove }
      : { kind: 'item', label: 'Delete Workspace…', disabled: true, hint: 'Default', title: 'The default workspace cannot be deleted' },
  ];
}

/** Agents whose transcript the daemon can read (their hooks name the file). OpenCode's plugin does not. */
const HAS_TRANSCRIPT: readonly AgentKind[] = ['claude_code', 'codex'];

/**
 * Session context menu: Open, Split right, Split down, Rename, Show Transcript, Show Changes, Stop.
 * Show Transcript needs an agent with a transcript file; Show Changes needs a git folder.
 */
export function sessionMenu(session: Session, actions: SessionMenuActions): MenuEntry[] {
  const transcript: MenuEntry = HAS_TRANSCRIPT.includes(session.agent)
    ? { kind: 'item', label: 'Show Transcript', onSelect: () => emitUiCommand('showTranscript', session.id) }
    : {
        kind: 'item',
        label: 'Show Transcript',
        disabled: true,
        hint: 'Unavailable',
        title: `Not available for ${AGENT_LABEL[session.agent]}`,
      };
  const changes: MenuEntry = session.git.is_git
    ? { kind: 'item', label: 'Show Changes', onSelect: () => emitUiCommand('showChanges', session.id) }
    : { kind: 'item', label: 'Show Changes', disabled: true, hint: 'No git', title: 'No git: this folder is not a git repository' };
  return [
    { kind: 'header', label: session.title },
    { kind: 'item', label: 'Open', hint: '⏎', onSelect: actions.open },
    { kind: 'item', label: 'Open in Split Right', hint: '⌥-click', onSelect: actions.split },
    { kind: 'item', label: 'Open in Split Down', hint: '⌥⇧-click', onSelect: actions.splitDown },
    { kind: 'separator' },
    { kind: 'item', label: 'Rename…', onSelect: actions.rename },
    transcript,
    changes,
    { kind: 'separator' },
    { kind: 'item', label: 'Stop Session', danger: true, onSelect: actions.stop },
  ];
}
