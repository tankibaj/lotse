import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { getSelectedSessionId, selectSession } from './selection';
import { OPEN_SESSION_EVENT, installNotificationOpen } from './notificationOpen';

// a notification body click arrives as a Tauri event; capture the handler `listen` is given
const handlers = new Map<string, (e: { payload: unknown }) => void>();
const unlisten = vi.fn();
vi.mock('@tauri-apps/api/event', () => ({
  listen: vi.fn(async (name: string, cb: (e: { payload: unknown }) => void) => {
    handlers.set(name, cb);
    return unlisten;
  }),
}));

const tauri = (on: boolean) => {
  const w = window as unknown as Record<string, unknown>;
  if (on) w.__TAURI_INTERNALS__ = {};
  else delete w.__TAURI_INTERNALS__;
};
const flush = () => new Promise<void>((r) => setTimeout(r, 0));

beforeEach(() => {
  handlers.clear();
  unlisten.mockClear();
  selectSession(null);
});
afterEach(() => tauri(false));

describe('notification click › open session', () => {
  it('selects the session the event names', async () => {
    tauri(true);
    installNotificationOpen();
    await flush();
    const fire = handlers.get(OPEN_SESSION_EVENT);
    expect(fire).toBeDefined();
    fire?.({ payload: 'a2' });
    expect(getSelectedSessionId()).toBe('a2');
  });

  it('ignores a payload that is not a session id', async () => {
    tauri(true);
    installNotificationOpen();
    await flush();
    selectSession('a1');
    handlers.get(OPEN_SESSION_EVENT)?.({ payload: null });
    handlers.get(OPEN_SESSION_EVENT)?.({ payload: '' });
    expect(getSelectedSessionId()).toBe('a1');
  });

  it('in the browser nothing is listened to', async () => {
    installNotificationOpen();
    await flush();
    expect(handlers.size).toBe(0);
  });

  it('stops listening, also when that happens before listen answered', async () => {
    tauri(true);
    const off = installNotificationOpen();
    off(); // before the pending `listen` promise resolves
    await flush();
    expect(unlisten).toHaveBeenCalledTimes(1);

    const again = installNotificationOpen();
    await flush();
    again();
    expect(unlisten).toHaveBeenCalledTimes(2);
  });
});
