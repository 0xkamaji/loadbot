import { defineConfig } from '@playwright/test';

export default defineConfig({
  testDir: './tests/browser',
  fullyParallel: true,
  use: {
    baseURL: 'http://127.0.0.1:1421',
    viewport: { width: 1000, height: 680 },
    // Restricted WSL1 validation hosts cannot launch Chromium's renderer processes.
    launchOptions: process.env.LOADBOT_BROWSER_SINGLE_PROCESS === '1'
      ? { args: ['--no-zygote', '--single-process', '--disable-gpu'] }
      : {},
  },
  // Own a dedicated test server; never accidentally test another native checkout
  // already listening on Tauri's normal development port (1420).
  webServer: { command: 'npm run dev -- --port 1421', url: 'http://127.0.0.1:1421', reuseExistingServer: false },
});
