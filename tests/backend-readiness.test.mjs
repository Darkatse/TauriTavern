import test from 'node:test';
import assert from 'node:assert/strict';
import path from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';

const REPO_ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
const READINESS_PATH = path.join(REPO_ROOT, 'src/tauri/main/bootstrap/backend-readiness.js');

async function importFreshReadiness() {
    const url = `${pathToFileURL(READINESS_PATH).href}?t=${Date.now()}-${Math.random()}`;
    return import(url);
}

function installFakeWindow({ invoke } = {}) {
    const calls = [];

    global.window = {
        __TAURI__: {
            core: {
                async invoke(command) {
                    calls.push(command);
                    return invoke?.(command);
                },
            },
        },
    };

    return { calls };
}

function cleanupGlobals() {
    delete global.window;
}

test('backend readiness waits on the explicit readiness command', async () => {
    const fake = installFakeWindow({
        invoke(command) {
            assert.equal(command, 'wait_for_backend_ready');
        },
    });

    try {
        const { waitForBackendReady } = await importFreshReadiness();
        await waitForBackendReady();

        assert.deepEqual(fake.calls, ['wait_for_backend_ready']);
    } finally {
        cleanupGlobals();
    }
});

test('backend readiness fails fast when Tauri invoke is unavailable', async () => {
    global.window = {};

    try {
        const { waitForBackendReady } = await importFreshReadiness();

        await assert.rejects(
            waitForBackendReady(),
            /Tauri invoke is unavailable for backend readiness/,
        );
    } finally {
        cleanupGlobals();
    }
});

test('backend readiness propagates backend startup failure', async () => {
    installFakeWindow({
        invoke() {
            throw new Error('backend failed');
        },
    });

    try {
        const { waitForBackendReady } = await importFreshReadiness();

        await assert.rejects(waitForBackendReady(), /backend failed/);
    } finally {
        cleanupGlobals();
    }
});
