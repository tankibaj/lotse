import { test } from '@playwright/test';
import type { Page } from '@playwright/test';
import { fileURLToPath, pathToFileURL } from 'node:url';
import { resolve } from 'node:path';

const HERE = fileURLToPath(new URL('.', import.meta.url));

// Screenshots the mock and the app at the same window size, light and dark (the Playwright project
// sets the OS scheme; the theme preference stays on Auto). Output: parity/polish/{mock,app}-<size>-<scheme>[-<surface>].png
// The mock normally floats in a padded page; its wrapper is stretched to the viewport (same size as the app).

const MOCK = pathToFileURL(resolve(HERE, '../../../spec/mock.html')).href;
const OUT = resolve(HERE, '../../../parity/polish');
const SIZES = [
  { w: 1440, h: 900 },
  { w: 1280, h: 800 },
];

/** The globals spec/mock.html defines (it is plain script, so they hang off `window`). */
type MockWindow = Window & { openSet(): void; openPal(): void; openNew(): void; openAdd(): void; lucide?: unknown };

type Surface = { name: string; mock: (p: Page) => Promise<void>; app: (p: Page) => Promise<void> };

const surfaces: Surface[] = [
  { name: 'main', mock: async () => {}, app: async () => {} },
  {
    name: 'settings',
    mock: (p) => p.evaluate(() => (window as unknown as MockWindow).openSet()),
    app: (p) => p.keyboard.press('Meta+,'),
  },
  {
    name: 'palette',
    mock: (p) => p.evaluate(() => (window as unknown as MockWindow).openPal()),
    app: (p) => p.keyboard.press('Meta+k'),
  },
  {
    name: 'new',
    mock: (p) => p.evaluate(() => (window as unknown as MockWindow).openNew()),
    app: (p) => p.keyboard.press('Meta+t'),
  },
  {
    name: 'add',
    mock: (p) => p.evaluate(() => (window as unknown as MockWindow).openAdd()),
    app: (p) => p.keyboard.press('Meta+Shift+o'),
  },
];

for (const { w, h } of SIZES) {
  for (const s of surfaces) {
    test(`mock ${w}x${h} ${s.name}`, async ({ page }, info) => {
      await page.setViewportSize({ width: w, height: h });
      await page.goto(MOCK);
      // Fill the viewport like the app's window, so the fixed overlays (palette, sheets) land at the same offsets.
      await page.addStyleTag({
        content: 'body{padding:0!important;background:none!important}#win{max-width:none!important;height:100vh!important;margin:0!important;border-radius:0!important;box-shadow:none!important}',
      });
      await page.waitForFunction(() => !!(window as unknown as MockWindow).lucide && document.querySelector('#tree .card'));
      await page.evaluate(() => document.fonts.ready);
      await s.mock(page);
      await page.waitForTimeout(200);
      await page.screenshot({ path: `${OUT}/mock-${w}-${info.project.name}${s.name === 'main' ? '' : '-' + s.name}.png`, animations: 'disabled' });
    });

    test(`app ${w}x${h} ${s.name}`, async ({ page }, info) => {
      await page.setViewportSize({ width: w, height: h });
      await page.goto('/');
      await page.getByTestId('shell').waitFor();
      await page.waitForTimeout(500);
      await s.app(page);
      await page.waitForTimeout(300);
      await page.screenshot({ path: `${OUT}/app-${w}-${info.project.name}${s.name === 'main' ? '' : '-' + s.name}.png`, animations: 'disabled' });
    });
  }
}
