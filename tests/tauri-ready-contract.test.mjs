import test from 'node:test';
import assert from 'node:assert/strict';
import path from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';

const REPO_ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
const READY_PATH = path.join(REPO_ROOT, 'src/scripts/extensions/runtime/tauri-ready.js');

async function importFreshReady() {
    const url = `${pathToFileURL(READY_PATH).href}?t=${Date.now()}-${Math.random()}`;
    return import(url);
}

function installTauriWindow(readyPromise) {
    global.window = {
        __TAURI_RUNNING__: true,
        __TAURITAVERN_MAIN_READY__: readyPromise,
    };
}

function rejectSoon(error) {
    return new Promise((_, reject) => {
        setTimeout(() => reject(error), 0);
    });
}

function cleanupGlobals() {
    delete global.window;
}

test('waitForTauriMainReady preserves default fallback behavior', async () => {
    installTauriWindow(rejectSoon(new Error('backend failed')));

    try {
        const { waitForTauriMainReady } = await importFreshReady();

        await assert.doesNotReject(waitForTauriMainReady());
    } finally {
        cleanupGlobals();
    }
});

test('waitForTauriMainReady can fail fast for the main startup path', async () => {
    installTauriWindow(rejectSoon(new Error('backend failed')));

    try {
        const { waitForTauriMainReady } = await importFreshReady();

        await assert.rejects(
            waitForTauriMainReady({ failFast: true }),
            /backend failed/,
        );
    } finally {
        cleanupGlobals();
    }
});
