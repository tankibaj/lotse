import { expect, test } from '@playwright/test';
import { fileURLToPath } from 'node:url';
import { resolve } from 'node:path';

// Interaction states and the Retina render of the app (the mock has no equivalent): hover, dialog focus ring,
// chrome cursor and selection. Output: parity/polish/app-retina-<scheme>[-<state>].png
// Chrome's DPR emulation still rounds 0.5 px borders up to 1 px (WKWebView does not), so hairlines are judged in the shots only.

const OUT = resolve(fileURLToPath(new URL('.', import.meta.url)), '../../../parity/polish');

test.use({ deviceScaleFactor: 2, viewport: { width: 1440, height: 900 } });

test('retina and states', async ({ page }, info) => {
  const s = info.project.name;
  await page.goto('/');
  await page.getByTestId('shell').waitFor();
  await page.waitForTimeout(500);
  await page.screenshot({ path: `${OUT}/app-retina-${s}.png`, animations: 'disabled' });

  const chrome = await page.evaluate(() => ({
    bodyCursor: getComputedStyle(document.body).cursor,
    userSelect: getComputedStyle(document.body).userSelect,
    glass: getComputedStyle(document.querySelector('[data-testid="sidebar-region"]')!).backdropFilter,
    smoothing: getComputedStyle(document.documentElement).getPropertyValue('-webkit-font-smoothing'),
  }));
  expect(chrome.bodyCursor).toBe('default');
  expect(chrome.userSelect).toBe('none');
  expect(chrome.glass).toContain('blur');

  await page.getByText('Flyway migration V42').first().hover();
  await page.waitForTimeout(300);
  await page.screenshot({ path: `${OUT}/app-retina-${s}-hover.png`, clip: { x: 0, y: 0, width: 520, height: 420 }, animations: 'disabled' });

  await page.keyboard.press('Meta+t');
  await page.getByRole('dialog', { name: 'New session' }).waitFor();
  await page.waitForTimeout(300);
  await page.screenshot({ path: `${OUT}/app-retina-${s}-dialog.png`, animations: 'disabled' });
});
