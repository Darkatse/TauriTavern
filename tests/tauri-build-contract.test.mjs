import test from 'node:test';
import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import { access, mkdir, writeFile } from 'node:fs/promises';
import path from 'node:path';
import process from 'node:process';
import { fileURLToPath } from 'node:url';

const REPO_ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');

test('pnpm Tauri wrapper builds clean frontend assets', async () => {
    const distDir = path.join(REPO_ROOT, 'src/dist');
    const staleBundle = path.join(distDir, 'lib.bundle.js');
    await mkdir(distDir, { recursive: true });
    await writeFile(staleBundle, 'stale bundle');

    const result = spawnSync(
        process.execPath,
        [path.join(REPO_ROOT, 'scripts/tauri-app.mjs'), '--prepare-frontend', '--help'],
        {
            cwd: REPO_ROOT,
            encoding: 'utf8',
            env: { ...process.env, TAURITAVERN_SKIP_WEB_BUILD: '0' },
        },
    );

    assert.equal(result.status, 0, result.stderr);
    await assert.rejects(access(staleBundle), (error) => error?.code === 'ENOENT');
    await access(path.join(distDir, 'lib.core.bundle.js'));
    await access(path.join(distDir, 'lib.optional.bundle.js'));
});

test('frontend build hook honors the explicit portable skip request', () => {
    const hookPath = path.join(REPO_ROOT, 'scripts/tauri-before-build.mjs');
    const result = spawnSync(process.execPath, [hookPath], {
        cwd: REPO_ROOT,
        env: { ...process.env, TAURITAVERN_SKIP_WEB_BUILD: '1' },
        encoding: 'utf8',
    });

    assert.equal(result.status, 0, result.stderr);
    assert.match(result.stdout, /Skipping frontend bundle build by request\./);
});
