import test from 'node:test';
import assert from 'node:assert/strict';
import { readFile, readdir } from 'node:fs/promises';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const REPO_ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');

async function readRepoFile(relativePath) {
    return readFile(path.join(REPO_ROOT, relativePath), 'utf8');
}

async function listJsFiles(relativeDir) {
    const root = path.join(REPO_ROOT, relativeDir);
    const results = [];
    const stack = [root];

    while (stack.length > 0) {
        const current = stack.pop();
        const entries = await readdir(current, { withFileTypes: true });
        for (const entry of entries) {
            const fullPath = path.join(current, entry.name);
            if (entry.isDirectory()) {
                stack.push(fullPath);
                continue;
            }
            if (entry.isFile() && entry.name.endsWith('.js')) {
                results.push(path.relative(REPO_ROOT, fullPath).replace(/\\/g, '/'));
            }
        }
    }

    return results.sort();
}

test('TauriTavern Dev Logs wrapper owns the host ABI boundary', async () => {
    const source = await readRepoFile('src/scripts/tauri/setting/dev-logs.js');

    assert.match(source, /window\.__TAURITAVERN__\?\.api\?\.dev/);
    assert.match(source, /dev-logs\.bundle\.js/);
    assert.match(source, /mountTauriTavernDevLogsApp/);
    assert.match(source, /openFullscreenTextViewer/);
    assert.match(source, /trimFrontendLogEntriesInPlace/);
    assert.doesNotMatch(source, /from\s+['"]vue(?:\/|['"])/);
    assert.doesNotMatch(source, /window\.__TAURI__/);
    assert.doesNotMatch(source, /devlog_/);
});

test('Rspack exposes a dedicated TauriTavern Dev Logs Vue entry', async () => {
    const source = await readRepoFile('rspack.config.js');

    assert.match(source, /['"]dev-logs['"]:\s*['"]\.\/src\/scripts\/tauri\/setting\/dev-logs-app\/index\.js['"]/);
    assert.match(source, /listJavaScriptFiles\(['"]src\/scripts\/tauri\/setting\/dev-logs-app['"]\)/);
    assert.match(source, /src\/scripts\/tauri\/setting\/dist/);
});

test('TauriTavern Dev Logs Vue app stays presentation-only', async () => {
    const files = await listJsFiles('src/scripts/tauri/setting/dev-logs-app');
    assert.ok(files.includes('src/scripts/tauri/setting/dev-logs-app/index.js'));
    assert.ok(files.includes('src/scripts/tauri/setting/dev-logs-app/LiveLogPanel.js'));
    assert.ok(files.includes('src/scripts/tauri/setting/dev-logs-app/LlmApiLogsPanel.js'));

    const forbidden = [
        'popup.js',
        'tauri-bridge.js',
        'text-viewer-popup.js',
        '__TAURITAVERN__',
        '__TAURI__',
        'devlog_',
    ];

    for (const file of files) {
        const source = await readRepoFile(file);
        for (const token of forbidden) {
            assert.doesNotMatch(source, new RegExp(token.replaceAll('.', '\\.')), `${file} contains ${token}`);
        }
    }

    const entry = await readRepoFile('src/scripts/tauri/setting/dev-logs-app/index.js');
    assert.match(entry, /from\s+['"]vue\/dist\/vue\.esm-bundler\.js['"]/);
    assert.match(entry, /export\s+function\s+mountTauriTavernDevLogsApp/);
});

test('TauriTavern Dev Logs Vue app owns subscription cleanup', async () => {
    const liveSource = await readRepoFile('src/scripts/tauri/setting/dev-logs-app/LiveLogPanel.js');
    const llmSource = await readRepoFile('src/scripts/tauri/setting/dev-logs-app/LlmApiLogsPanel.js');

    assert.match(liveSource, /this\.unsubscribe\s*=\s*await\s+this\.client\.subscribe/);
    assert.match(liveSource, /unmounted\(\)\s*\{[\s\S]*this\.unsubscribe\?\.\(\)/);
    assert.match(llmSource, /this\.unsubscribe\s*=\s*await\s+this\.client\.subscribeIndex/);
    assert.match(llmSource, /unmounted\(\)\s*\{[\s\S]*this\.unsubscribe\?\.\(\)/);
});
