import type { DaemonClient } from '../client/DaemonClient';

/** Bell in the status bar / ⌥⌘N: pause or resume all notifications (read-modify-write on the daemon's settings). */
export async function togglePausedNotifications(client: DaemonClient): Promise<void> {
  const s = await client.getSettings();
  await client.updateSettings({ ...s, notifications: { ...s.notifications, paused: !s.notifications.paused } });
}
