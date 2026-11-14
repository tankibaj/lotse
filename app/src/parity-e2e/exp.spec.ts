import { expect, test } from '@playwright/test';

// Vibrancy, simulated: in Tauri the shell sets `data-vibrancy` on <html> and the window is transparent over the macOS
// sidebar material. Here a gradient on <html> stands in for the material; the sidebar and status bar must show it,
// the main area must stay solid. Output: parity/polish/app-vibrancy-<scheme>.png

test('vibrancy css lets the material through the sidebar only', async ({ page }, info) => {
  await page.setViewportSize({ width: 1440, height: 900 });
  await page.goto('/');
  await page.getByTestId('shell').waitFor();
  await page.evaluate(() => {
    document.documentElement.dataset.vibrancy = '';
    document.documentElement.style.background = 'linear-gradient(135deg, #ff5f6d, #4facfe)';
  });
  await page.waitForTimeout(300);
  const alpha = (sel: string) => page.evaluate((s) => getComputedStyle(document.querySelector(s)!).backgroundColor, sel);
  expect(await alpha('[data-testid="sidebar-region"]')).toMatch(/color\(srgb[^)]*\/ 0\.62\)|rgba\(.*0\.62\)/);
  expect(await alpha('main')).not.toMatch(/\/|rgba/);
  await page.screenshot({ path: `../parity/polish/app-vibrancy-${info.project.name}.png`, animations: 'disabled' });
});
