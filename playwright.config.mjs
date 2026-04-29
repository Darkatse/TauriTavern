import { defineConfig, devices } from '@playwright/test';

export default defineConfig({
    testDir: './tests/a11y',
    fullyParallel: false,
    workers: 1,
    reporter: 'list',
    timeout: 30_000,
    use: {
        ...devices['Desktop Chrome'],
        headless: true,
        trace: 'retain-on-failure',
    },
});
