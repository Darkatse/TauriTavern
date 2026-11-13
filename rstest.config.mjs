import { defineConfig } from '@rstest/core';

import { pluginTestFrontend } from './tests/helpers/frontend-server.mjs';

export default defineConfig({
  include: ['{packages,src}/**/*.test.{ts,tsx}', 'tests/webview/*.test.mjs'],
  plugins: [pluginTestFrontend()],
  browser: { enabled: true, provider: 'playwright', headless: true },
  setupFiles: ['./tests/helpers/rstest-setup.mjs'],
  // A hung browser page must fail its test instead of holding the job until the workflow timeout.
  testTimeout: 30_000,
  restoreMocks: true,
  unstubGlobals: true,
  tools: {
    swc: {
      jsc: {
        transform: {
          react: {
            runtime: 'automatic',
          },
        },
      },
    },
  },
});
