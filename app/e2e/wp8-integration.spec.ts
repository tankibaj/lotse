import { expect, test } from '@playwright/test';
import type { Page } from '@playwright/test';

// WP8 UI wiring against the web build (dev server, MockClient): sidebar menu items that were disabled
// until the UI commands existed, ⌥-click split, and the mock parity shots for them.
// Start the dev server first: `npm run dev -- --port 1421`, run with LOTSE_E2E_URL=http://127.0.0.1:1421.
// Mock shots come from spec/mock.html (file://); screenshots go to ../parity/wp8.

const SHOTS = '../parity/wp8';
const MOCK = new URL('../../spec/mock.html', import.meta.url);
const SIZES = [
  { width: 1440, height: 900 },
  { width: 1280, height: 800 },
];

test.use({ channel: process.env.LOTSE_E2E_BUNDLED ? undefined : 'chrome', baseURL: process.env.LOTSE_E2E_URL ?? 'http://127.0.0.1:1421' });

async function openApp(page: Page, theme: string, size: { width: number; height: number }) {
  await page.setViewportSize(size);
  await page.addInitScript((t) => localStorage.setItem('lotse-theme', t), theme);
  await page.emulateMedia({ reducedMotion: 'reduce' });
  await page.clock.install();
  await page.goto('/');
  await expect(page.getByTestId('sidebar-region').getByText('billing-api')).toBeVisible();
}

async function openMock(page: Page, theme: string, size: { width: number; height: number }) {
  await page.setViewportSize(size);
  await page.addInitScript((t) => localStorage.setItem('lotse-theme', t), theme);
  await page.emulateMedia({ reducedMotion: 'reduce' });
  await page.goto(MOCK.href, { waitUntil: 'load' });
  await page.waitForTimeout(800);
}

const CLIP = { x: 0, y: 0, width: 620, height: 700 };
const tag = (theme: string, size: { width: number }) => `${theme}-${size.width}`;

test('web build: project menu items (icon & colour, New Session with project), session menu (Split Right), ⌥-click split', async ({ page }) => {
  await openApp(page, 'light', SIZES[0]!);

  // project menu: Change Icon & Color opens the popover under the row; Reveal in Finder needs the app
  await page.getByRole('button', { name: /billing-api/ }).first().click({ button: 'right' });
  await expect(page.getByRole('menuitem', { name: /Reveal in Finder/ })).toHaveAttribute('aria-disabled', 'true');
  await page.getByRole('menuitem', { name: /Change Icon & Color/ }).click();
  const swatches = page.getByRole('group', { name: 'Colour' });
  await expect(swatches).toBeVisible();
  await page.keyboard.press('Escape');
  await expect(swatches).toBeHidden();

  // New Session from the project menu preselects that project
  await page.getByRole('button', { name: /infra-terraform/ }).first().click({ button: 'right' });
  await page.getByRole('menuitem', { name: /New Session/ }).click();
  await expect(page.getByRole('dialog', { name: /New session/i }).getByRole('combobox').first()).toHaveValue(/p\d+|infra/);
  await page.keyboard.press('Escape');

  // session menu: Split Right opens that session beside the focused pane
  await expect(page.locator('[data-pane]')).toHaveCount(1);
  await page.locator('[data-sid="a2"]').click({ button: 'right' });
  await page.getByRole('menuitem', { name: /Split Right/ }).click();
  await expect(page.locator('[data-pane]')).toHaveCount(2);
  await expect(page.locator('[data-pane="a2"]')).toBeVisible();

  // ⌥-click on a card splits too (third pane)
  await page.locator('[data-sid="a3"]').click({ modifiers: ['Alt'] });
  await expect(page.locator('[data-pane]')).toHaveCount(3);
});

for (const theme of ['light', 'dark'] as const) {
  for (const size of SIZES) {
    test(`parity ${tag(theme, size)}: project menu, session menu, split`, async ({ page }) => {
      await openApp(page, theme, size);
      await page.getByRole('button', { name: /billing-api/ }).first().click({ button: 'right' });
      await expect(page.getByRole('menuitem', { name: /Change Icon & Color/ })).toBeVisible();
      await page.screenshot({ path: `${SHOTS}/app-project-menu-${tag(theme, size)}.png`, clip: CLIP });
      await page.keyboard.press('Escape');
      await page.locator('[data-sid="a2"]').click({ button: 'right' });
      await page.screenshot({ path: `${SHOTS}/app-session-menu-${tag(theme, size)}.png`, clip: CLIP });
      await page.getByRole('menuitem', { name: /Split Right/ }).click();
      await expect(page.locator('[data-pane]')).toHaveCount(2);
      await page.mouse.move(700, 450);
      await page.screenshot({ path: `${SHOTS}/app-split-${tag(theme, size)}.png` });
    });

    test(`mock ${tag(theme, size)}: project menu, session menu, split`, async ({ page }) => {
      await openMock(page, theme, size);
      await page.locator('#tree button[aria-expanded]', { hasText: 'billing-api' }).click({ button: 'right' });
      await page.screenshot({ path: `${SHOTS}/mock-project-menu-${tag(theme, size)}.png`, clip: CLIP });
      await page.keyboard.press('Escape');
      await page.evaluate(() => (window as unknown as { openCardCtx(id: string, x: number, y: number): void }).openCardCtx('a2', 120, 220));
      await page.screenshot({ path: `${SHOTS}/mock-session-menu-${tag(theme, size)}.png`, clip: CLIP });
      await page.evaluate(() => (window as unknown as { closeCtx(): void; splitWith(id: string): void }).splitWith('a2'));
      await page.mouse.move(700, 450);
      await page.screenshot({ path: `${SHOTS}/mock-split-${tag(theme, size)}.png` });
    });
  }
}
