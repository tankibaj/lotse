import { defineConfig } from '@playwright/test';
import base from './playwright.config';

// Same walk against the installed Google Chrome, for machines where `npx playwright install` was not run:
// `cd app && npx playwright test -c src/sheets/e2e/playwright.chrome.config.ts`
export default defineConfig({ ...base, use: { ...base.use, channel: 'chrome' } });
