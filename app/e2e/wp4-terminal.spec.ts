import { existsSync } from 'node:fs';
import { chromium, expect, test } from '@playwright/test';
import type { Locator, Page } from '@playwright/test';

// WP4 e2e against `npm run dev -- --port 1422` (MockClient, no daemon). Not part of CI.
//   LOTSE_E2E_URL=http://127.0.0.1:1422 npx playwright test e2e/wp4-terminal.spec.ts
// The DOM renderer is forced (localStorage) so terminal text can be read; the WebGL test turns it off.
// Playwright's own Chromium not downloaded (no network at install time)? Use the installed Chrome.
// `LOTSE_E2E_CHANNEL` overrides the choice.
const channel = process.env.LOTSE_E2E_CHANNEL ?? (existsSync(chromium.executablePath()) ? undefined : 'chrome');
test.use({
  baseURL: process.env.LOTSE_E2E_URL ?? 'http://127.0.0.1:1422',
  viewport: { width: 1440, height: 900 },
  channel,
});

const panes = (page: Page) => page.locator('[data-pane]');
const focusedPane = (page: Page) => page.locator('[data-pane][data-focused="true"]');
const rows = (pane: Locator) => pane.locator('.xterm-rows');

async function open(page: Page, renderer: 'dom' | 'webgl' = 'dom') {
  await page.addInitScript((r) => {
    if (r === 'dom') localStorage.setItem('lotse-term-renderer', 'dom');
  }, renderer);
  await page.goto('/');
  if (renderer === 'dom') await expect(rows(focusedPane(page))).toContainText('Claude Code');
  else await expect(focusedPane(page).locator('.xterm-screen canvas').last()).toBeVisible();
}

test('typing round-trips through the MockClient echo session', async ({ page }) => {
  await open(page);
  await focusedPane(page).locator('.xterm-helper-textarea').focus();
  await page.keyboard.type('echo hello-lotse');
  await expect(rows(focusedPane(page))).toContainText('echo hello-lotse');
  await page.keyboard.press('Enter');
  await page.keyboard.type('second line');
  await expect(rows(focusedPane(page))).toContainText('second line');
});

test('tabs switch between the sessions of the project and replay each screen', async ({ page }) => {
  await open(page);
  const tabs = page.getByRole('tab');
  await expect(tabs).toHaveCount(2);
  await tabs.nth(1).click();
  await expect(tabs.nth(1)).toHaveAttribute('aria-selected', 'true');
  await expect(rows(focusedPane(page))).toContainText('Flyway migration V42');
  await tabs.nth(0).click();
  await expect(rows(focusedPane(page))).toContainText('Claude Code');
});

test('keyboard: arrows move through tabs repeatedly, Ctrl+Tab leaves the terminal', async ({ page }) => {
  await open(page);
  const tabs = page.getByRole('tab');
  await tabs.nth(0).focus();
  await page.keyboard.press('ArrowRight');
  await expect(tabs.nth(1)).toBeFocused();
  await page.keyboard.press('ArrowLeft'); // a second arrow still reaches the tab list, not the terminal
  await expect(tabs.nth(0)).toBeFocused();
  await expect(tabs.nth(0)).toHaveAttribute('aria-selected', 'true');

  await focusedPane(page).locator('.xterm-helper-textarea').focus();
  await page.keyboard.press('Tab'); // belongs to the shell: focus stays
  await expect(focusedPane(page).locator('.xterm-helper-textarea')).toBeFocused();
  await page.keyboard.press('Control+Tab');
  await expect(focusedPane(page).locator('.xterm-helper-textarea')).not.toBeFocused();
  expect(await page.evaluate(() => document.activeElement?.closest('.xterm') === null)).toBe(true);
});

test('closing an unfocused pane with its ✕ leaves the keyboard in the terminal', async ({ page }) => {
  await open(page);
  await page.keyboard.press('Meta+d');
  await expect(panes(page)).toHaveCount(2);
  await panes(page).nth(0).getByRole('button', { name: 'Close pane' }).click();
  await expect(panes(page)).toHaveCount(1);
  await expect(focusedPane(page).locator('.xterm-helper-textarea')).toBeFocused();
});

test('⌘D splits, ⌘[ ⌘] move focus, ⌘W closes, four panes at most', async ({ page }) => {
  await open(page);
  await expect(panes(page)).toHaveCount(1);
  await page.keyboard.press('Meta+d');
  await expect(panes(page)).toHaveCount(2);
  const second = await focusedPane(page).getAttribute('data-pane');
  await page.keyboard.press('Meta+[');
  await expect(focusedPane(page)).toHaveAttribute('data-pane', 'a1');
  await page.keyboard.press('Meta+]');
  await expect(focusedPane(page)).toHaveAttribute('data-pane', second ?? '');

  await page.keyboard.press('Meta+d');
  await page.keyboard.press('Meta+d');
  await expect(panes(page)).toHaveCount(4);
  await page.keyboard.press('Meta+d');
  await expect(panes(page)).toHaveCount(4);
  await expect(page.getByRole('status')).toHaveText('Up to 4 panes side by side');

  await page.keyboard.press('Meta+w');
  await expect(panes(page)).toHaveCount(3);
  await page.getByRole('button', { name: 'Close pane' }).first().click();
  await expect(panes(page)).toHaveCount(2);
});

test('dragging a divider resizes the neighbours and respects the minimum width', async ({ page }) => {
  await open(page);
  await page.keyboard.press('Meta+d');
  await expect(panes(page)).toHaveCount(2);
  const [left, right] = [panes(page).nth(0), panes(page).nth(1)];
  const w0 = (await left.boundingBox())?.width ?? 0;
  const divider = page.getByRole('separator', { name: 'Resize panes' });
  const box = await divider.boundingBox();
  if (!box) throw new Error('no divider');
  const y = box.y + box.height / 2;
  await page.mouse.move(box.x + 0.5, y);
  await page.mouse.down();
  await page.mouse.move(box.x + 0.5 + 200, y, { steps: 5 });
  await page.mouse.up();
  const w1 = (await left.boundingBox())?.width ?? 0;
  expect(w1).toBeGreaterThan(w0 + 150);
  expect((await right.boundingBox())?.width ?? 0).toBeLessThan(w0 - 150);

  await page.mouse.move(box.x + 200, y);
  await page.mouse.down();
  await page.mouse.move(0, y, { steps: 5 });
  await page.mouse.up();
  expect((await left.boundingBox())?.width ?? 0).toBeGreaterThanOrEqual(219);
});

test('dropping a session card on the terminal opens it side by side', async ({ page }) => {
  await open(page);
  await page.evaluate(() => {
    const area = document.querySelector('[data-testid="panes"]');
    const dataTransfer = new DataTransfer();
    dataTransfer.setData('text/lotse-session', 'a4');
    const fire = (type: string) => area?.dispatchEvent(new DragEvent(type, { dataTransfer, bubbles: true, cancelable: true }));
    fire('dragover');
  });
  await expect(page.getByTestId('drop-hint')).toBeVisible();
  await page.evaluate(() => {
    const area = document.querySelector('[data-testid="panes"]');
    const dataTransfer = new DataTransfer();
    dataTransfer.setData('text/lotse-session', 'a4');
    area?.dispatchEvent(new DragEvent('drop', { dataTransfer, bubbles: true, cancelable: true }));
  });
  await expect(panes(page)).toHaveCount(2);
  await expect(panes(page).nth(1)).toHaveAttribute('data-pane', 'a4');
  await expect(page.getByTestId('drop-hint')).toHaveCount(0);
});

test('⌘+ and ⌘− change the terminal font size', async ({ page }) => {
  await open(page);
  const size = () => rows(focusedPane(page)).evaluate((el) => getComputedStyle(el).fontSize);
  expect(await size()).toBe('13px');
  await page.keyboard.press('Meta+=');
  await expect.poll(size).toBe('14px');
  await page.keyboard.press('Meta+-');
  await page.keyboard.press('Meta+-');
  await expect.poll(size).toBe('12px');
  await page.keyboard.press('Meta+0');
  await expect.poll(size).toBe('13px');
});

test('terminal follows light and dark theme', async ({ page }) => {
  await open(page);
  const bg = () => page.locator('.xterm-scrollable-element').first().evaluate((el) => getComputedStyle(el).backgroundColor);
  expect(await bg()).toBe('rgb(255, 255, 255)');
  await page.evaluate(() => document.documentElement.classList.add('dark'));
  await expect.poll(bg).toBe('rgb(30, 30, 30)');
});

// The WebGL canvas is the one xterm sizes to the screen element without a class (the others are the link layer
// and a 64x48 glyph measuring canvas). Unverified at 2x: Playwright's `deviceScaleFactor` emulation changes
// `window.devicePixelRatio` but not the screen ratio the renderer draws at, so only 1x can be checked headless.
test('WebGL renderer is mounted and sized to the screen at 1x', async ({ page }) => {
  await open(page, 'webgl');
  const sizes = await focusedPane(page).evaluate((pane) => {
    const screen = pane.querySelector<HTMLElement>('.xterm-screen');
    const gl = [...pane.querySelectorAll<HTMLCanvasElement>('.xterm-screen canvas')].find((c) => c.className === '' && c.style.width !== '');
    return { screenCss: screen?.getBoundingClientRect().width ?? 0, glDevice: gl?.width ?? 0, glCss: gl?.getBoundingClientRect().width ?? 0, dpr: devicePixelRatio };
  });
  expect(sizes.glDevice).toBeGreaterThan(0);
  expect(Math.abs(sizes.glCss - sizes.screenCss)).toBeLessThan(2);
  expect(Math.abs(sizes.glDevice - sizes.glCss * sizes.dpr)).toBeLessThan(2);
});
