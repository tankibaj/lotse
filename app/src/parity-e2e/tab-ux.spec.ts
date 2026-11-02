import { expect, test } from '@playwright/test';
import { fileURLToPath } from 'node:url';
import { resolve } from 'node:path';

const HERE = fileURLToPath(new URL('.', import.meta.url));

// Tab-strip UX in a real browser: the "+" dropdown, the tab context menu and drag-to-reorder, light and dark
// (the Playwright project sets the OS scheme). Output: parity/tab-ux/<shot>-<scheme>.png
// Run with the parity config: `cd app && npx playwright test -c src/parity-e2e/playwright.config.ts tab-ux`

const OUT = resolve(HERE, '../../../parity/tab-ux');

test.describe('tab UX', () => {
  test.use({ viewport: { width: 1440, height: 900 } });

  test.beforeEach(async ({ page }) => {
    await page.goto('/');
    await page.getByTestId('shell').waitFor();
    await page.getByRole('tab').first().waitFor();
    await page.waitForTimeout(500);
  });

  test('"+" opens the agent dropdown', async ({ page }, info) => {
    await page.getByRole('button', { name: 'New session', exact: true }).click();
    const menu = page.getByRole('menu', { name: 'New session' });
    await expect(menu.getByRole('menuitem')).toHaveText([/New Terminal/, /Claude Code/, /Codex/, /OpenCode/, /New session…/, /Agent settings…/]);
    await expect(menu.getByRole('menuitem').first()).toBeFocused();
    await page.waitForTimeout(200);
    await page.screenshot({ path: `${OUT}/plus-menu-${info.project.name}.png`, animations: 'disabled' });
    await page.keyboard.press('Escape');
    await expect(menu).toBeHidden();
  });

  test('right-click on a tab shows the session menu, not the browser one', async ({ page }, info) => {
    await page.getByRole('tab').nth(1).click({ button: 'right' });
    await expect(page.getByRole('menu', { name: /actions$/ })).toBeVisible();
    await page.waitForTimeout(200);
    await page.screenshot({ path: `${OUT}/tab-menu-${info.project.name}.png`, animations: 'disabled' });
    await page.keyboard.press('Escape');
  });

  test('dragging the third tab to the front reorders it', async ({ page }, info) => {
    await page.getByRole('button', { name: 'New session', exact: true }).click();
    await page.getByRole('menuitem', { name: /New Terminal/ }).click();
    const tabs = page.getByRole('tab');
    await expect(tabs).toHaveCount(3);
    const first = (await tabs.nth(0).boundingBox())!;
    const third = (await tabs.nth(2).boundingBox())!;
    await page.mouse.move(third.x + third.width / 2, third.y + third.height / 2);
    await page.mouse.down();
    await page.mouse.move(first.x + 10, first.y + first.height / 2, { steps: 12 });
    await expect(page.getByTestId('tab-drop-indicator')).toBeVisible();
    await page.screenshot({ path: `${OUT}/tab-drag-${info.project.name}.png`, animations: 'disabled' });
    await page.mouse.up();
    await expect(tabs.nth(0)).toContainText('Terminal');
    await expect(tabs.nth(0)).toHaveAttribute('aria-selected', 'true');
    await expect(page.getByTestId('tab-drop-indicator')).toHaveCount(0);
  });
});
