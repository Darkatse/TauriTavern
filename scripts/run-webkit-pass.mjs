import { execFileSync } from 'node:child_process';
import { createRequire } from 'node:module';
import path from 'node:path';

// Playwright WebKit never starts a test page on the Windows CI runner: its pass stays at zero completed
// files until the job times out, while the same files finish in seconds locally. Chromium still covers
// Windows, and Linux and macOS still run WebKit, so skip only on that runner.
if (process.env.CI && process.platform === 'win32') {
    console.log('Skipping the WebKit pass: Playwright WebKit does not start pages on the Windows CI runner.');
} else {
    const require = createRequire(import.meta.url);
    const core = path.dirname(require.resolve('@rstest/core/package.json'));
    const cli = path.join(core, require('@rstest/core/package.json').bin.rstest);
    execFileSync(process.execPath, [cli, '--browser.name', 'webkit'], { stdio: 'inherit' });
}
