import { test, type Page } from '@playwright/test';

// WP9 parity: the mock's Settings › Agents and › Notifications next to the app's (MockClient), light and
// dark, 1440x900, plus both main views after the scripted status changes. Shots go to ../parity/wp9.
// The mock loads Tailwind and Lucide from a CDN (dev-time only, not the app).

test.use({ channel: process.env.LOTSE_E2E_BUNDLED ? undefined : 'chrome' });

const MOCK = new URL('../../spec/mock.html', import.meta.url);
const SHOTS = '../parity/wp9';

async function prepare(page: Page, theme: 'light' | 'dark') {
  await page.setViewportSize({ width: 1440, height: 900 });
  await page.addInitScript((t) => localStorage.setItem('lotse-theme', t), theme);
  // the app's theme setting comes from the daemon (MockClient: auto), so also follow the OS scheme
  await page.emulateMedia({ reducedMotion: 'reduce', colorScheme: theme });
}

for (const theme of ['light', 'dark'] as const) {
  test(`mock ${theme}`, async ({ page }) => {
    await prepare(page, theme);
    await page.goto(MOCK.href, { waitUntil: 'load' });
    await page.waitForTimeout(800);
    await page.screenshot({ path: `${SHOTS}/mock-${theme}-main.png` });
    for (const pane of ['Agents', 'Notifications']) {
      await page.evaluate((p) => (window as unknown as { openSet: (k: string) => void }).openSet(p), pane);
      await page.waitForTimeout(200);
      await page.screenshot({ path: `${SHOTS}/mock-${theme}-${pane.toLowerCase()}.png` });
    }
  });

  test(`app ${theme}`, async ({ page }) => {
    await prepare(page, theme);
    await page.goto('/');
    await page.waitForTimeout(800);
    await page.screenshot({ path: `${SHOTS}/app-${theme}-main.png` });
    await page.keyboard.press('Meta+,');
    for (const pane of ['Agents', 'Notifications']) {
      await page.getByRole('button', { name: pane, exact: true }).first().click();
      await page.waitForTimeout(200);
      await page.screenshot({ path: `${SHOTS}/app-${theme}-${pane.toLowerCase()}.png` });
    }
  });
}
