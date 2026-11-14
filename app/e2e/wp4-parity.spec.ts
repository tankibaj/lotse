import { existsSync } from 'node:fs';
import { pathToFileURL } from 'node:url';
import { resolve } from 'node:path';
import { chromium, expect, test } from '@playwright/test';
import type { Page } from '@playwright/test';

// Mock parity screenshots for WP4: the same view of spec/mock.html and of the app (MockClient), side by side.
// Writes parity/wp4/<viewport>-<state>-<theme>-<mock|app>.png: only the tab row, panes and terminal
// (the `<main>` element) are compared; the sidebar and status bar belong to WP3. The mock loads Tailwind and Lucide from a CDN: needs network.
//   npx playwright test e2e/wp4-parity.spec.ts   (dev server on 1422)
const channel = process.env.LOTSE_E2E_CHANNEL ?? (existsSync(chromium.executablePath()) ? undefined : 'chrome');
test.use({ baseURL: process.env.LOTSE_E2E_URL ?? 'http://127.0.0.1:1422', channel });

const OUT = resolve(process.cwd(), '../parity/wp4');
const MOCK = pathToFileURL(resolve(process.cwd(), '../spec/mock.html')).href;
type Theme = 'light' | 'dark';
const VIEWPORTS = [
  { name: '1440x900', width: 1440, height: 900, states: ['single', 'split2', 'split4'] },
  { name: '1280x800', width: 1280, height: 800, states: ['split2'] },
] as const;

/** Both layouts keep tabs and panes in `<main>`; the sidebar and status bar (WP3) are out of the shot. */
async function shoot(page: Page, name: string) {
  await page.locator('main').screenshot({ path: `${OUT}/${name}.png` });
}

for (const vp of VIEWPORTS) {
  for (const theme of ['light', 'dark'] as Theme[]) {
    for (const state of vp.states) {
      const splits = state === 'single' ? 0 : state === 'split2' ? 1 : 3;
      const base = `${vp.name}-${state}-${theme}`;

      test(`mock ${base}`, async ({ page }) => {
        await page.setViewportSize({ width: vp.width, height: vp.height });
        await page.addInitScript((t) => localStorage.setItem('lotse-theme', t), theme);
        await page.goto(MOCK);
        await page.waitForFunction(() => typeof (window as unknown as { splitWith?: unknown }).splitWith === 'function');
        for (let i = 0; i < splits; i++) await page.evaluate(() => (window as unknown as { splitWith: (id: null) => void }).splitWith(null));
        await expect(page.locator('#panes .pane')).toHaveCount(splits + 1);
        await shoot(page, `${base}-mock`);
      });

      // DOM renderer: exact fractional cell widths like the mock's HTML text. The WebGL renderer rounds cells
      // to whole device pixels (7 px instead of 7.8 px at 1x), so headless 1x shots would differ for that reason only.
      test(`app ${base}`, async ({ page }) => {
        await page.setViewportSize({ width: vp.width, height: vp.height });
        await page.addInitScript((t) => {
          localStorage.setItem('lotse-theme', t);
          localStorage.setItem('lotse-term-renderer', 'dom');
        }, theme);
        await page.goto('/');
        await expect(page.locator('[data-pane] .xterm-rows').first()).toContainText('Claude Code');
        for (let i = 0; i < splits; i++) await page.keyboard.press('Meta+d');
        await expect(page.locator('[data-pane]')).toHaveCount(splits + 1);
        await expect(page.locator('[data-pane] .xterm-rows').last()).not.toBeEmpty();
        await page.waitForTimeout(300); // replays finish writing
        await shoot(page, `${base}-app`);
      });
    }
  }
}
