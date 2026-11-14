import { defineConfig } from '@playwright/test';

// Not part of CI. Used for manual parity screenshots: `npm run dev -- --port 1421` first.
export default defineConfig({
  testDir: 'e2e',
  use: { baseURL: 'http://127.0.0.1:1421' },
});
