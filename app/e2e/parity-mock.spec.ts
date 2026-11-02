import { test } from '@playwright/test';

// Screenshots of spec/mock.html (file://) for the parity gate, next to the app's in ../parity/wp3.
// The mock loads Tailwind and Lucide from a CDN: this only renders styled with network access (dev-time, not the app).

test.use({ channel: process.env.LOTSE_E2E_BUNDLED ? undefined : 'chrome' });

const MOCK = new URL('../../spec/mock.html', import.meta.url);

for (const theme of ['light', 'dark'] as const) {
  test(`mock ${theme} 1440x900`, async ({ page }) => {
    await page.setViewportSize({ width: 1440, height: 900 });
    await page.addInitScript((t) => localStorage.setItem('lotse-theme', t), theme);
    await page.emulateMedia({ reducedMotion: 'reduce' });
    await page.goto(MOCK.href, { waitUntil: 'load' });
    await page.waitForTimeout(800);
    await page.screenshot({ path: `../parity/wp3/mock-${theme}-1440-default.png` });
    // collapse the same three projects as the app's "collapsed" shot
    for (const n of ['infra-terraform', 'dotfiles', 'praxis-web']) await page.locator('#tree button[aria-expanded]', { hasText: n }).click();
    await page.mouse.move(700, 400);
    await page.screenshot({ path: `../parity/wp3/mock-${theme}-1440-collapsed.png` });
  });
}
