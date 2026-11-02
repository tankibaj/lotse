import type { DaemonClient } from '../client/DaemonClient';
import type { AgentKind, Project, Session } from '../client/types';
import { emitUiCommand } from '../ui/commands';
import { getSelectedSessionId, selectSession } from '../ui/selection';

/** Bell in the status bar / ⌥⌘N: pause or resume all notifications (read-modify-write on the daemon's settings). */
export async function togglePausedNotifications(client: DaemonClient): Promise<void> {
  const s = await client.getSettings();
  await client.updateSettings({ ...s, notifications: { ...s.notifications, paused: !s.notifications.paused } });
}

/**
 * Start a session running `agent` (no worktree, no prompt) in the selected session's project, else the first project,
 * and select it. The "+" menu and ⌘T / ⌥⌘T use it. With no project at all it opens Add project instead.
 */
export async function startSession(
  client: DaemonClient,
  workspace: { projects: readonly Project[]; sessions: readonly Session[] },
  agent: AgentKind,
): Promise<Session | null> {
  const current = workspace.sessions.find((s) => s.id === getSelectedSessionId());
  const projectId = current?.project_id ?? workspace.projects[0]?.id;
  if (!projectId) {
    emitUiCommand('openAddProject');
    return null;
  }
  const created = await client.createSession({
    project_id: projectId,
    agent,
    command: null, // the daemon applies the launch command from Settings
    title: null,
    worktree: null,
    first_prompt: null,
  });
  selectSession(created.id);
  return created;
}
