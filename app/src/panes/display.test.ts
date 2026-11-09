import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

const monitor = vi.hoisted(() => ({ next: null as unknown, fail: false }));
vi.mock('@tauri-apps/api/window', () => ({
  currentMonitor: async () => {
    if (monitor.fail) throw new Error('no window');
    return monitor.next;
  },
}));

import { currentDisplay } from './display';
import { DISPLAY_FALLBACK_NAME } from './layout';

const logical = (width: number, height: number) => ({ toLogical: () => ({ width, height }) });
const at = (x: number, y: number) => ({ toLogical: () => ({ x, y }) });

beforeEach(() => {
  (window as unknown as Record<string, unknown>).__TAURI_INTERNALS__ = {};
  monitor.fail = false;
});
afterEach(() => {
  delete (window as unknown as Record<string, unknown>).__TAURI_INTERNALS__;
});

describe('currentDisplay (review 10)', () => {
  it('uses one fallback name for a missing monitor name, a missing monitor and a failed lookup', async () => {
    monitor.next = { name: null, scaleFactor: 2, size: logical(1512, 982), position: at(0, 0) };
    const nameless = await currentDisplay();
    monitor.next = null;
    const missing = await currentDisplay();
    monitor.fail = true;
    const failed = await currentDisplay();
    expect([nameless.name, missing.name, failed.name]).toEqual([DISPLAY_FALLBACK_NAME, DISPLAY_FALLBACK_NAME, DISPLAY_FALLBACK_NAME]);
  });

  it('reports scale and position of the monitor', async () => {
    monitor.next = { name: 'Studio Display', scaleFactor: 2, size: logical(2560, 1440), position: at(1512, -300) };
    expect(await currentDisplay()).toEqual({ name: 'Studio Display', width: 2560, height: 1440, scale: 2, x: 1512, y: -300 });
  });
});
