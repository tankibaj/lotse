import '@testing-library/jest-dom/vitest';
import { afterEach } from 'vitest';
import { cleanup } from '@testing-library/react';
import { PERM_SHOW_DELAY } from './lib/state/store';

// Tests see a permission request at once; the delay has its own tests.
PERM_SHOW_DELAY.ms = 0;

afterEach(() => {
  cleanup();
  document.documentElement.classList.remove('dark');
  try {
    localStorage.clear();
  } catch {
    // no storage in this environment
  }
});
