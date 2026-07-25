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

// Returns a controllable deferred rejection. Rejecting via a bare
// setTimeout(0) races the dynamic import() in the tests: on slower
// filesystems (e.g. Windows) the promise rejects before any handler is
// attached, and the test runner fails the test on the unhandled rejection.
function createDeferredRejection() {
    let rejectPromise;
    const promise = new Promise((_, reject) => {
        rejectPromise = reject;
    });
    return {
        promise,
        reject: (error) => rejectPromise(error),
    };
}

function cleanupGlobals() {
    delete global.window;
}

test('waitForTauriMainReady preserves default fallback behavior', async () => {
    const deferred = createDeferredRejection();
    installTauriWindow(deferred.promise);

    try {
        const { waitForTauriMainReady } = await importFreshReady();

        const waiting = waitForTauriMainReady();
        deferred.reject(new Error('backend failed'));
        await assert.doesNotReject(waiting);
    } finally {
        cleanupGlobals();
    }
});

test('waitForTauriMainReady can fail fast for the main startup path', async () => {
    const deferred = createDeferredRejection();
    installTauriWindow(deferred.promise);

    try {
        const { waitForTauriMainReady } = await importFreshReady();

        const waiting = waitForTauriMainReady({ failFast: true });
        deferred.reject(new Error('backend failed'));
        await assert.rejects(
            waiting,
            /backend failed/,
        );
    } finally {
        cleanupGlobals();
    }
});
