import test from 'node:test';
import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import { access, mkdir, readFile, writeFile } from 'node:fs/promises';
import path from 'node:path';
import process from 'node:process';
import { fileURLToPath } from 'node:url';

const REPO_ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');

test('Tauri builds use the repository frontend build hook', async () => {
    const config = JSON.parse(await readFile(
        path.join(REPO_ROOT, 'src-tauri/crates/tauritavern/tauri.conf.json'),
        'utf8',
    ));

    assert.deepEqual(config.build.beforeBuildCommand, {
        script: 'node scripts/tauri-before-build.mjs',
        cwd: '../../..',
    });
});

test('pnpm Tauri entrypoints prepare frontend assets', async () => {
    const { scripts } = JSON.parse(await readFile(path.join(REPO_ROOT, 'package.json'), 'utf8'));
    const entrypoints = [
        'tauri',
        'android',
        'ios',
        'tauri:dev',
        'tauri:dev:pilot',
        'tauri:build',
        'android:dev',
        'android:build',
        'ios:dev',
        'ios:build',
    ];

    for (const entrypoint of entrypoints) {
        assert.match(scripts[entrypoint], /(?:^|\s)--prepare-frontend(?:\s|$)/u, entrypoint);
    }
});

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

test('portable builds delegate frontend ownership to the Tauri hook', async () => {
    const source = await readFile(path.join(REPO_ROOT, 'scripts/build-portable.mjs'), 'utf8');

    assert.doesNotMatch(source, /run\("pnpm", \["run", "web:build"\]/);
    assert.match(source, /TAURITAVERN_SKIP_WEB_BUILD: "1"/);
});

test('Canary release workflow does not build frontend assets twice', async () => {
    const workflow = await readFile(
        path.join(REPO_ROOT, '.github/workflows/canary-release.yml'),
        'utf8',
    );

    assert.doesNotMatch(workflow, /run:\s+pnpm run web:build/u);
    assert.match(workflow, /run:\s+node scripts\/build-portable\.mjs --skip-web-build/u);
    assert.doesNotMatch(workflow, /args:.*--no-bundle --features portable/u);
    assert.match(workflow, /date \+'%Y\.%m\.%d'/u);
    assert.match(workflow, /--title "Canary Release \$DISPLAY_TIME"/u);
});

test('Canary release notes isolate Codex skills and keep a deterministic fallback', async () => {
    const workflow = await readFile(
        path.join(REPO_ROOT, '.github/workflows/canary-release.yml'),
        'utf8',
    );

    assert.match(workflow, /cp -R \.github\/codex\/skills\/\. "\$codex_home\/skills\/"/u);
    assert.match(workflow, /codex-home: \$\{\{ steps\.codex-home\.outputs\.path \}\}/u);
    assert.match(workflow, /permission-profile: ":read-only"/u);
    assert.match(workflow, /cp context\/fallback\.md release-notes\.md/u);
    assert.doesNotMatch(workflow, /\.agents\/skills|models\.github\.ai/u);
});
