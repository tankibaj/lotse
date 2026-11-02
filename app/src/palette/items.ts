import type { Project, Session, Settings, Status } from '../lib/client/types';

/** Roll-up order from the plan: perm > fail > run > done > idle (unknown last). */
export const STATUS_RANK: Record<Status, number> = { perm: 5, fail: 4, run: 3, done: 2, idle: 1, unknown: 0 };

export type PaletteAction =
  | { type: 'session'; sessionId: string }
  | { type: 'project'; projectId: string }
  | { type: 'command'; id: 'next-need' | 'add-project' | 'new-session' | 'settings' | 'shortcuts' | 'pause' };

export interface PaletteItem {
  key: string;
  label: string;
  hint: string;
  status?: Status;
  /** Leading mark for commands. */
  glyph?: string;
  action: PaletteAction;
}

const AGENT_LABEL = { terminal: 'Terminal', claude_code: 'Claude Code', codex: 'Codex', opencode: 'OpenCode' } as const;

export const COMMANDS: { id: Extract<PaletteAction, { type: 'command' }>['id']; label: string; hint: string; glyph: string }[] = [
  { id: 'next-need', label: 'Next session that needs you', hint: '⌘J', glyph: '▲' },
  { id: 'add-project', label: 'Add a Project…', hint: '⇧⌘O', glyph: '+' },
  { id: 'new-session', label: 'New session…', hint: '⌘T', glyph: '+' },
  { id: 'settings', label: 'Settings…', hint: '⌘,', glyph: '⚙︎' },
  { id: 'shortcuts', label: 'Keyboard shortcuts', hint: '⌘/', glyph: '⌘' },
  { id: 'pause', label: 'Pause notifications', hint: '⌥⌘N', glyph: '○' },
];

/** The sessions that need attention first (`perm`, then `fail`): what ⌘J jumps to. */
export function nextNeedingYou(sessions: Session[], currentId: string | null): Session | null {
  const need = sessions
    .filter((s) => s.status === 'perm' || s.status === 'fail')
    .sort((a, b) => STATUS_RANK[b.status] - STATUS_RANK[a.status] || b.last_activity - a.last_activity);
  if (!need.length) return null;
  const at = need.findIndex((s) => s.id === currentId);
  return need[(at + 1) % need.length] ?? null;
}

/**
 * Everything the palette can jump to: sessions (most urgent first), projects, commands.
 * `query` matches label and hint, case-insensitive, like the mock.
 */
export function paletteItems(query: string, projects: Project[], sessions: Session[], settings?: Settings): PaletteItem[] {
  const name = new Map(projects.map((p) => [p.id, p.name]));
  const sessionItems: PaletteItem[] = [...sessions]
    .sort((a, b) => STATUS_RANK[b.status] - STATUS_RANK[a.status] || b.last_activity - a.last_activity)
    .map((s) => ({
      key: `s:${s.id}`,
      label: `${s.title} · ${name.get(s.project_id) ?? '?'}`,
      hint: [AGENT_LABEL[s.agent], s.git.is_git ? s.git.branch : 'No git'].join(' · '),
      status: s.status,
      action: { type: 'session', sessionId: s.id },
    }));
  const projectItems: PaletteItem[] = projects.map((p) => ({
    key: `p:${p.id}`,
    label: p.name,
    hint: 'Project',
    glyph: '▸',
    action: { type: 'project', projectId: p.id },
  }));
  const commandItems: PaletteItem[] = COMMANDS.map((c) => ({
    key: `c:${c.id}`,
    label: c.id === 'pause' && settings?.notifications.paused ? 'Resume notifications' : c.label,
    hint: c.hint,
    glyph: c.glyph,
    action: { type: 'command', id: c.id },
  }));
  const q = query.trim().toLowerCase();
  return [...sessionItems, ...commandItems, ...projectItems].filter((i) => !q || `${i.label} ${i.hint}`.toLowerCase().includes(q));
}
