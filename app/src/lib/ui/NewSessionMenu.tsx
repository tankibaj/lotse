import type { ReactNode } from 'react';
import { openSettings } from '../../sheets/overlay';
import { ContextMenu } from '../../sidebar/ContextMenu';
import type { MenuEntry } from '../../sidebar/ContextMenu';
import { useClient } from '../client/context';
import type { AgentKind, ProjectId } from '../client/types';
import { startSession } from '../state/actions';
import { useWorkspace } from '../state/store';
import { emitUiCommand } from './commands';

// Monochrome 16 px glyphs in the mock's icon style (1.4 stroke, currentColor). No brand logos.
const Glyph = ({ d, circles }: { d: string; circles?: [number, number][] }) => (
  <svg width="14" height="14" viewBox="0 0 16 16" fill="none" stroke="currentColor" strokeWidth="1.4" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
    <path d={d} />
    {circles?.map(([cx, cy]) => <circle key={`${cx}${cy}`} cx={cx} cy={cy} r="1.5" />)}
  </svg>
);
const ICON: Record<AgentKind | 'session' | 'settings', ReactNode> = {
  terminal: <Glyph d="M2 3.5h12v9H2zM4.5 6.5 6.5 8l-2 1.5M8 10h3" />,
  claude_code: <Glyph d="M8 2l1.5 4.5L14 8l-4.5 1.5L8 14l-1.5-4.5L2 8l4.5-1.5z" />,
  codex: <Glyph d="M6 5 3 8l3 3M10 5l3 3-3 3" />,
  opencode: <Glyph d="M5.5 3H4v10h1.5M10.5 3H12v10h-1.5" />,
  session: <Glyph d="M8 3.5v9M3.5 8h9" />,
  settings: <Glyph d="M2.5 5h5M12.5 5h1M2.5 11h1M8.5 11h5" circles={[[10, 5], [6, 11]]} />,
};

/**
 * The new-session dropdown (tab strip "+", sidebar "New session" link and project menu): a terminal or an agent in
 * `projectId` (default: the current session's project, else the first), or the full sheet for a worktree or another project.
 * `heading` names the project when the menu is not obviously about the current one.
 */
export function NewSessionMenu({ x, y, projectId, heading, onClose }: {
  x: number;
  y: number;
  projectId?: ProjectId;
  heading?: string;
  onClose: () => void;
}) {
  const client = useClient();
  const { projects, sessions } = useWorkspace();
  const start = (agent: AgentKind) => void startSession(client, { projects, sessions }, agent, projectId).catch(() => {});
  const entries: MenuEntry[] = [
    ...(heading ? [{ kind: 'header', label: heading } as const] : []),
    { kind: 'item', icon: ICON.terminal, label: 'New Terminal', hint: '⌘T', onSelect: () => start('terminal') },
    { kind: 'separator' },
    { kind: 'item', icon: ICON.claude_code, label: 'Claude Code', hint: '⌥⌘T', onSelect: () => start('claude_code') },
    { kind: 'item', icon: ICON.codex, label: 'Codex', onSelect: () => start('codex') },
    { kind: 'item', icon: ICON.opencode, label: 'OpenCode', onSelect: () => start('opencode') },
    { kind: 'separator' },
    { kind: 'item', icon: ICON.session, label: 'Worktree or other project…', onSelect: () => emitUiCommand('openNewSession', projectId) },
    { kind: 'item', icon: ICON.settings, label: 'Agent settings…', onSelect: () => openSettings('Agents') },
  ];
  return <ContextMenu x={x} y={y} label="New session" entries={entries} onClose={onClose} />;
}
