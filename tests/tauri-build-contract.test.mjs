import test from 'node:test';
import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import { access, mkdir, readFile, writeFile } from 'node:fs/promises';
import path from 'node:path';
import process from 'node:process';
import { fileURLToPath } from 'node:url';

import { createRspackConfigs } from '../rspack.config.js';

const REPO_ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');

test('Rspack exposes one mode-aware build graph for production and development', () => {
    const production = createRspackConfigs('production');
    const development = createRspackConfigs('development');
    const names = ['vendor-libs', 'agent-system', 'mcp-manager', 'tauritavern-settings'];

    assert.deepEqual(production.map(config => config.name), names);
    assert.deepEqual(development.map(config => config.name), names);

    for (const [index, config] of production.entries()) {
        assert.equal(config.mode, 'production');
        assert.equal(config.bail, true);
        assert.equal(config.cache.type, 'persistent');
        assert.equal('version' in config.cache, false);
        assert.match(config.cache.storage.directory, new RegExp(`${names[index]}$`, 'u'));
    }

    for (const config of development) {
        assert.equal(config.mode, 'development');
        assert.equal(config.bail, false);
        assert.equal(config.cache.type, 'memory');
    }

    for (const name of names.slice(1)) {
        const productionConfig = production.find(config => config.name === name);
        const developmentConfig = development.find(config => config.name === name);
        const productionReact = productionConfig.module.rules[0].use.options.jsc.transform.react;
        const developmentReact = developmentConfig.module.rules[0].use.options.jsc.transform.react;

        assert.deepEqual(productionReact, { development: false, runtime: 'automatic' });
        assert.deepEqual(developmentReact, { development: true, runtime: 'automatic' });
    }
});

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

test('Android Wry overrides remain app-owned and follow the generated ABI', async () => {
    const androidRoot = path.join(
        REPO_ROOT,
        'src-tauri/crates/tauritavern/gen/android/app',
    );
    const [ignore, gradle, viewClient, chromeClient] = await Promise.all([
        readFile(path.join(androidRoot, '.gitignore'), 'utf8'),
        readFile(path.join(androidRoot, 'build.gradle.kts'), 'utf8'),
        readFile(path.join(
            androidRoot,
            'src/main/java/com/tauritavern/client/RustWebViewClient.kt',
        ), 'utf8'),
        readFile(path.join(
            androidRoot,
            'src/main/java/com/tauritavern/client/RustWebChromeClient.kt',
        ), 'utf8'),
    ]);

    assert.match(ignore, /^\/src\/main\/\*\*\/generated$/mu);
    assert.match(gradle, /generated\/RustWebViewClient\.kt/u);
    assert.match(gradle, /generated\/RustWebChromeClient\.kt/u);
    assert.match(viewClient, /Baseline: wry 0\.55\.1/u);
    assert.match(viewClient, /webView:\s*RustWebView,\s*context:\s*Context,/u);
    assert.match(viewClient, /Rust\.handleRequest\(view\.id,/u);
    assert.match(viewClient, /equals\("Cache-Control",\s*ignoreCase\s*=\s*true\)/u);
    assert.doesNotMatch(viewClient, /external fun/u);
    assert.match(chromeClient, /Baseline: wry 0\.55\.1/u);
    assert.match(chromeClient, /Rust\.handleReceivedTitle\(/u);
});

test('Android support floor stays aligned with the Tauri runtime dependency', async () => {
    const config = JSON.parse(await readFile(
        path.join(REPO_ROOT, 'src-tauri/crates/tauritavern/tauri.conf.json'),
        'utf8',
    ));
    const gradle = await readFile(path.join(
        REPO_ROOT,
        'src-tauri/crates/tauritavern/gen/android/app/build.gradle.kts',
    ), 'utf8');

    assert.equal(config.bundle.android.minSdkVersion, 26);
    assert.match(gradle, /\bminSdk\s*=\s*26\b/u);
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

test('pnpm Tauri development delegates bundles to the watched dev server', async () => {
    const staleBundle = path.join(REPO_ROOT, 'src/dist/lib.bundle.js');
    await mkdir(path.dirname(staleBundle), { recursive: true });
    await writeFile(staleBundle, 'development sentinel');

    const result = spawnSync(
        process.execPath,
        [path.join(REPO_ROOT, 'scripts/tauri-app.mjs'), '--prepare-frontend', 'dev', '--help'],
        {
            cwd: REPO_ROOT,
            encoding: 'utf8',
            env: { ...process.env, TAURITAVERN_SKIP_WEB_BUILD: '0' },
        },
    );

    assert.equal(result.status, 0, result.stderr);
    assert.equal(await readFile(staleBundle, 'utf8'), 'development sentinel');
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
