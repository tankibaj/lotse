import { defineConfig } from '@playwright/test';

// Every spec in this folder against the one dev server of the integration run:
//   npm run dev -- --port 1421   then   npx playwright test -c e2e/playwright.all.config.ts
// The WP4 specs default to port 1422; this points them at 1421 without editing them. Not part of CI.
process.env.LOTSE_E2E_URL ??= 'http://127.0.0.1:1421';

export default defineConfig({
  testDir: '.',
  testMatch: '*.spec.ts',
  workers: 1,
  use: { baseURL: process.env.LOTSE_E2E_URL },
});
