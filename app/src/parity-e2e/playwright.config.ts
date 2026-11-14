import { defineConfig } from '@playwright/test';

// Parity shots: `cd app && npx playwright test -c src/parity-e2e/playwright.config.ts`
// Screenshots spec/mock.html and the app (dev server, port 1424, MockClient) into parity/polish/.
// Not part of CI. The mock loads Tailwind and lucide from a CDN, so this needs network (the app does not).
const PORT = 1424;

export default defineConfig({
  testDir: '.',
  testMatch: '*.spec.ts',
  workers: 1,
  timeout: 60_000,
  use: { baseURL: `http://127.0.0.1:${PORT}`, channel: 'chrome', reducedMotion: 'reduce' },
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
