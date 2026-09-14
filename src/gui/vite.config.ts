import { readFileSync } from 'node:fs';
import { defineConfig } from 'vitest/config';
import react from '@vitejs/plugin-react';

export default defineConfig({
  plugins: [react(), {
    name: 'bundled-font-license',
    generateBundle() {
      this.emitFile({
        type: 'asset',
        fileName: 'licenses/LICENSE-DejaVu.txt',
        source: readFileSync(new URL('./loadbot-gui-assets/fonts/LICENSE-DejaVu.txt', import.meta.url)),
      });
    },
  }],
  clearScreen: false,
  server: { port: 1420, strictPort: true },
  // The separate browser embedding entry is deliberately dev-only.
  build: { target: 'es2022' },
  test: {
    environment: 'jsdom',
    setupFiles: ['./tests/setup.ts'],
    include: ['tests/**/*.test.tsx'],
  },
});
