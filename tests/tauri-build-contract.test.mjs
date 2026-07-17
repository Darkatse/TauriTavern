import test from 'node:test';
import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import { readFile } from 'node:fs/promises';
import path from 'node:path';
import process from 'node:process';
import { fileURLToPath } from 'node:url';

const REPO_ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');

test('Tauri builds run the repository frontend build hook', async () => {
    const config = JSON.parse(await readFile(
        path.join(REPO_ROOT, 'src-tauri/crates/tauritavern/tauri.conf.json'),
        'utf8',
    ));
    const hookSource = await readFile(path.join(REPO_ROOT, 'scripts/tauri-before-build.mjs'), 'utf8');

    assert.deepEqual(config.build.beforeBuildCommand, {
        script: 'node scripts/tauri-before-build.mjs',
        cwd: '../../..',
    });
    assert.match(hookSource, /TAURITAVERN_SKIP_WEB_BUILD === "1"/);
    assert.match(hookSource, /spawnSync\("pnpm run web:build"/);
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

test('portable builds delegate frontend ownership to the Tauri hook', async () => {
    const source = await readFile(path.join(REPO_ROOT, 'scripts/build-portable.mjs'), 'utf8');

    assert.doesNotMatch(source, /run\("pnpm", \["run", "web:build"\]/);
    assert.match(source, /TAURITAVERN_SKIP_WEB_BUILD: "1"/);
    assert.match(source, /run\("node", tauriArgs, repoRoot, tauriEnv\);/);
});
