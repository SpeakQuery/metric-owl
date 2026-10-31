import { defineConfig } from '@playwright/test';

export default defineConfig({
  testDir: 'tests',
  testMatch: 'overview.spec.ts',
  workers: 1,
  use: { baseURL: 'http://127.0.0.1:4899', screenshot: 'only-on-failure' },
  webServer: {
    command: 'pnpm exec vite --host 127.0.0.1 --port 4899',
    url: 'http://127.0.0.1:4899/admin/',
    reuseExistingServer: false,
  },
});
