import { readFileSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
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
  server: {
    port: 1420,
    strictPort: true,
    watch: {
      // Tauri watches Rust sources; Vite must not watch Cargo's locked Windows DLLs.
      ignored: ['**/src-tauri/**'],
    },
  },
  // The separate browser embedding entry is deliberately dev-only.
  build: {
    target: 'es2022',
    rollupOptions: {
      // Tauri owns desktop.html; index.html remains the ordinary browser entry.
      input: {
        desktop: fileURLToPath(new URL('./desktop.html', import.meta.url)),
        index: fileURLToPath(new URL('./index.html', import.meta.url)),
      },
    },
  },
  test: {
    environment: 'jsdom',
    setupFiles: ['./tests/setup.ts'],
    include: ['tests/**/*.test.{ts,tsx}'],
  },
});
