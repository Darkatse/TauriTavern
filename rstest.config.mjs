import { defineConfig } from '@rstest/core';

export default defineConfig({
  include: ['src/scripts/extensions/**/*.test.tsx'],
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
