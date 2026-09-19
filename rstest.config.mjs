import { defineConfig } from '@rstest/core';

export default defineConfig({
  include: [
    'src/scripts/extensions/agent-system/src/**/*.test.{ts,tsx}',
    'src/scripts/extensions/mcp-manager/src/**/*.test.{ts,tsx}',
    'src/scripts/tauri/setting/**/*.test.{ts,tsx}',
  ],
  testEnvironment: 'happy-dom',
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
