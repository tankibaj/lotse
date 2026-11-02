import { expect, test } from '@playwright/test';
import type { Locator, Page } from '@playwright/test';
import { fileURLToPath } from 'node:url';
import { resolve } from 'node:path';

const HERE = fileURLToPath(new URL('.', import.meta.url));

// The new-session dropdown (tab "+", sidebar project menu) and the title bar's one centre line (y = 22 px), sidebar
// shown and hidden, light and dark. Output: parity/new-session-menu/<shot>-<scheme>.png
// Run with the parity config: `cd app && npx playwright test -c src/parity-e2e/playwright.config.ts new-session-menu`

const OUT = resolve(HERE, '../../../parity/new-session-menu');
const LINE = 22;

const centre = async (l: Locator) => {
  const b = await l.boundingBox();
  if (!b) throw new Error('not visible');
  return b.y + b.height / 2;
};

test.describe('new-session menu and title bar', () => {
  test.use({ viewport: { width: 1440, height: 900 } });

  test.beforeEach(async ({ page }) => {
    await page.goto('/');
    await page.getByTestId('shell').waitFor();
    await page.getByRole('tab').first().waitFor();
    await page.waitForTimeout(500);
  });

  const onLine = async (page: Page, extra: Locator[]) => {
    const tabLabel = page.getByRole('tab').first().locator('span.truncate');
    const targets = [tabLabel, page.getByRole('button', { name: 'New session', exact: true }), page.getByRole('button', { name: 'Session', exact: true }), ...extra];
    for (const t of targets) expect(Math.abs((await centre(t)) - LINE)).toBeLessThanOrEqual(1);
  };

  test('sidebar shown: title, gear, sidebar icon, tab label, "+" and toolbar share the centre line', async ({ page }, info) => {
    await onLine(page, [
      page.getByText('Lotse', { exact: true }),
      page.getByRole('button', { name: 'Settings' }),
      page.getByRole('button', { name: /Hide sidebar/ }),
    ]);
    await page.screenshot({ path: `${OUT}/sidebar-shown-${info.project.name}.png`, animations: 'disabled' });
  });

  test('sidebar hidden: the toggle is the sidebar icon, right after the window controls, on the same line', async ({ page }, info) => {
    await page.getByRole('button', { name: /Hide sidebar/ }).click();
    const show = page.getByRole('button', { name: 'Show sidebar' });
    await expect(show.locator('svg')).toHaveCount(1);
    await expect(show).not.toContainText('☰');
    await onLine(page, [show]);
    // in the browser the mock's dots stand in for the native lights: the toggle starts where the sidebar title does (80 px)
    expect(Math.round((await show.boundingBox())!.x)).toBe(80);
    await page.screenshot({ path: `${OUT}/sidebar-hidden-${info.project.name}.png`, animations: 'disabled' });
  });

  test('"+" menu: one active item, hover moves it', async ({ page }, info) => {
    await page.getByRole('button', { name: 'New session', exact: true }).click();
    const menu = page.getByRole('menu', { name: 'New session' });
    await expect(menu.getByRole('menuitem', { name: /Worktree or other project…/ })).toBeVisible();
    await expect(menu.getByRole('menuitem').first()).toBeFocused();
    await page.screenshot({ path: `${OUT}/plus-menu-${info.project.name}.png`, animations: 'disabled' });
    await menu.getByRole('menuitem', { name: /^OpenCode/ }).hover();
    await expect(menu.getByRole('menuitem', { name: /^OpenCode/ })).toBeFocused();
    // exactly one item is painted (the highlight is a background colour; the rest are transparent)
    const painted = await menu.getByRole('menuitem').evaluateAll((els) => els.filter((e) => getComputedStyle(e).backgroundColor !== 'rgba(0, 0, 0, 0)').length);
    expect(painted).toBe(1);
    await page.screenshot({ path: `${OUT}/plus-menu-hover-${info.project.name}.png`, animations: 'disabled' });
  });

  test('project menu › New Session… opens the dropdown for that project', async ({ page }, info) => {
    await page.getByRole('button', { name: /^notes/ }).click({ button: 'right' });
    await page.getByRole('menuitem', { name: /New Session/ }).click();
    const menu = page.getByRole('menu', { name: 'New session' });
    await expect(menu.getByText('notes', { exact: true })).toBeVisible();
    await page.waitForTimeout(200);
    await page.screenshot({ path: `${OUT}/sidebar-menu-${info.project.name}.png`, animations: 'disabled' });
    await menu.getByRole('menuitem', { name: /^New Terminal/ }).click();
    // the new terminal belongs to `notes`: its tab strip shows it
    await expect(page.getByRole('tablist')).toHaveAccessibleName('Sessions in notes');
  });
});
