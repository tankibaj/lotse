import { defineConfig } from '@playwright/test';

// WP5 walk: `cd app && npx playwright test -c src/sheets/e2e/playwright.config.ts`
// Runs against the dev server on port 1424 (MockClient), once in light and once in dark.
// Not part of CI (the app's own e2e config uses another port).
const PORT = 1424;

export default defineConfig({
  testDir: '.',
  testMatch: '*.spec.ts',
  workers: 1,
  use: { baseURL: `http://127.0.0.1:${PORT}`, viewport: { width: 1440, height: 900 } },
  webServer: {
    command: `npm run dev -- --port ${PORT}`,
    url: `http://127.0.0.1:${PORT}`,
    reuseExistingServer: true,
  },
  projects: [
    { name: 'light', use: { colorScheme: 'light' } },
    { name: 'dark', use: { colorScheme: 'dark' } },
  ],
});
