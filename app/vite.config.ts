/// <reference types="vitest/config" />
import { defineConfig } from 'vite';
import react from '@vitejs/plugin-react';
import tailwindcss from '@tailwindcss/vite';

// Dev server: loopback only, dev only. The shipped app has no listener (Tauri serves bundled assets).
// `npm run dev -- --port 1421` overrides VITE_PORT.
const port = Number(process.env.VITE_PORT ?? 1420);

export default defineConfig({
  plugins: [react(), tailwindcss()],
  clearScreen: false,
  server: {
    host: '127.0.0.1',
    port,
    strictPort: true,
    // The MockClient seeds from the shared proto fixture in crates/lotse-proto/fixtures.
    fs: { allow: ['..'] },
  },
  build: { target: 'safari16', sourcemap: false },
  test: {
    environment: 'jsdom',
    globals: true,
    setupFiles: ['./src/test-setup.ts'],
    include: ['src/**/*.test.{ts,tsx}'],
  },
});
