import { expect, test } from '@playwright/test';
import type { Page } from '@playwright/test';
import { fileURLToPath } from 'node:url';
import { resolve } from 'node:path';

const HERE = fileURLToPath(new URL('.', import.meta.url));

// Split down in a real browser: ⌘D / ⌘⇧D, the drop zones, the stack divider and the xterm refit, light and dark
// (the Playwright project sets the OS scheme). Output: parity/split-down/<shot>-<scheme>.png
// Run with the parity config: `cd app && npx playwright test -c src/parity-e2e/playwright.config.ts split-down`

const OUT = resolve(HERE, '../../../parity/split-down');

/** The three-pane layout of the shots: one pane on the left, two stacked on the right. */
async function rightThenDown(page: Page) {
  await page.keyboard.press('Meta+d');
  await expect(page.locator('[data-pane]')).toHaveCount(2);
  await page.keyboard.press('Meta+Shift+D');
  await expect(page.locator('[data-pane]')).toHaveCount(3);
  await expect(page.locator('[data-column]')).toHaveCount(2);
  await expect(page.getByTestId('pane-stack-divider')).toHaveCount(1);
  await expect(page.getByTestId('pane-divider')).toHaveCount(1);
  await page.waitForTimeout(400);
}

test.describe('split down', () => {
  test.use({ viewport: { width: 1440, height: 900 } });

  test.beforeEach(async ({ page }) => {
    await page.goto('/');
    await page.getByTestId('shell').waitFor();
    await page.getByRole('tab').first().waitFor();
    await page.locator('[data-pane] .xterm').first().waitFor();
    await page.waitForTimeout(500);
  });

  test('⌘D then ⌘⇧D: a column on the left, two stacked panes on the right', async ({ page }, info) => {
    await rightThenDown(page);
    const [left, topRight, bottomRight] = await page.locator('[data-pane]').all();
    const a = (await left!.boundingBox())!;
    const b = (await topRight!.boundingBox())!;
    const c = (await bottomRight!.boundingBox())!;
    expect(b.x).toBeGreaterThan(a.x + a.width - 2); // beside
    expect(c.x).toBeCloseTo(b.x, 0); // same column
    expect(c.y).toBeGreaterThan(b.y + b.height - 2); // below
    expect(c.width).toBeCloseTo(b.width, 0);
    await expect(page.locator('[data-pane][data-focused="true"]')).toHaveCount(1);
    await page.screenshot({ path: `${OUT}/right-down-${info.project.name}.png`, animations: 'disabled' });
  });

  test('dragging a session card over a pane shows the right or bottom half, and dropping splits that way', async ({ page }, info) => {
    const card = page.locator('[data-session-id]').last();
    const pane = (await page.locator('[data-pane]').first().boundingBox())!;
    const from = (await card.boundingBox())!;
    await page.mouse.move(from.x + from.width / 2, from.y + from.height / 2);
    await page.mouse.down();
    await page.mouse.move(pane.x + pane.width * 0.85, pane.y + pane.height * 0.3, { steps: 12 });
    await expect(page.getByTestId('drop-hint')).toHaveAttribute('data-zone', 'right');
    await page.screenshot({ path: `${OUT}/drop-zone-right-${info.project.name}.png`, animations: 'disabled' });
    await page.mouse.move(pane.x + pane.width * 0.3, pane.y + pane.height * 0.85, { steps: 12 });
    await expect(page.getByTestId('drop-hint')).toHaveAttribute('data-zone', 'down');
    await page.screenshot({ path: `${OUT}/drop-zone-down-${info.project.name}.png`, animations: 'disabled' });
    await page.mouse.up();
    await expect(page.getByTestId('drop-hint')).toHaveCount(0);
    await expect(page.locator('[data-pane]')).toHaveCount(2);
    await expect(page.locator('[data-column]')).toHaveCount(1); // stacked in one column
  });

  test('dragging a tab down onto the bottom half of the pane stacks its session', async ({ page }, info) => {
    const tab = page.getByRole('tab').nth(1);
    const pane = (await page.locator('[data-pane]').first().boundingBox())!;
    const from = (await tab.boundingBox())!;
    await page.mouse.move(from.x + from.width / 2, from.y + from.height / 2);
    await page.mouse.down();
    await page.mouse.move(pane.x + pane.width * 0.3, pane.y + pane.height * 0.85, { steps: 12 });
    await expect(page.getByTestId('drop-hint')).toHaveAttribute('data-zone', 'down');
    await page.screenshot({ path: `${OUT}/tab-drop-down-${info.project.name}.png`, animations: 'disabled' });
    await page.mouse.up();
    await expect(page.locator('[data-pane]')).toHaveCount(2);
    await expect(page.locator('[data-column]')).toHaveCount(1);
    await expect(page.locator('[data-pane][data-focused="true"]')).toHaveCount(1);
  });

  test('dragging the stack divider resizes the panes and xterm refits to the new height', async ({ page }, info) => {
    await rightThenDown(page);
    const screenHeight = async (nth: number) => (await page.locator('[data-pane]').nth(nth).locator('.xterm-screen').boundingBox())!.height;
    const before = await screenHeight(1);
    const divider = (await page.getByTestId('pane-stack-divider').boundingBox())!;
    await expect(page.getByTestId('pane-stack-divider')).toHaveCSS('cursor', 'row-resize');
    await page.mouse.move(divider.x + divider.width / 2, divider.y + 0.5);
    await page.mouse.down();
    await page.mouse.move(divider.x + divider.width / 2, divider.y - 150, { steps: 10 });
    await page.mouse.up();
    await page.waitForTimeout(300);
    const after = await screenHeight(1);
    expect(after).toBeLessThan(before - 60);
    expect(Number(await page.getByTestId('pane-stack-divider').getAttribute('aria-valuenow'))).toBeLessThan(50);
    await page.screenshot({ path: `${OUT}/resized-${info.project.name}.png`, animations: 'disabled' });
  });

  test('closing the lower pane collapses the stack', async ({ page }) => {
    await rightThenDown(page);
    await page.keyboard.press('Meta+w');
    await expect(page.locator('[data-pane]')).toHaveCount(2);
    await expect(page.getByTestId('pane-stack-divider')).toHaveCount(0);
    await expect(page.locator('[data-column]')).toHaveCount(2);
  });
});
