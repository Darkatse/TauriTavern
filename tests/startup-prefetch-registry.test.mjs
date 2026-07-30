import test from 'node:test';
import assert from 'node:assert/strict';
import path from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';

const REPO_ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
const REGISTRY_PATH = path.join(REPO_ROOT, 'src/scripts/tauri/startup/startup-prefetch-registry.js');

async function importFreshRegistry() {
    return import(`${pathToFileURL(REGISTRY_PATH).href}?t=${Date.now()}-${Math.random()}`);
}

test('startup prefetch reuses a pending load once', async () => {
    const { startStartupPrefetch, consumeStartupPrefetch } = await importFreshRegistry();
    let loadCount = 0;

    const first = startStartupPrefetch('demo', async () => {
        loadCount += 1;
        return 'prefetched';
    });
    const second = startStartupPrefetch('demo', async () => 'unused');

    assert.equal(first, second);
    assert.equal(await consumeStartupPrefetch('demo', async () => 'fallback'), 'prefetched');
    assert.equal(loadCount, 1);
    assert.equal(await consumeStartupPrefetch('demo', async () => 'fallback'), 'fallback');
});

test('startup prefetch shares a pending consume across concurrent consumers', async () => {
    const { startStartupPrefetch, consumeStartupPrefetch } = await importFreshRegistry();
    let resolvePrefetch;
    let fallbackCount = 0;

    startStartupPrefetch('demo', () => new Promise(resolve => {
        resolvePrefetch = resolve;
    }));
    await Promise.resolve();

    const first = consumeStartupPrefetch('demo', async () => {
        fallbackCount += 1;
        return 'fallback';
    });
    const second = consumeStartupPrefetch('demo', async () => {
        fallbackCount += 1;
        return 'fallback';
    });

    resolvePrefetch('prefetched');

    assert.deepEqual(await Promise.all([first, second]), ['prefetched', 'prefetched']);
    assert.equal(fallbackCount, 0);
});

test('startup prefetch retries the real loader after a prefetch failure', async () => {
    const { startStartupPrefetch, consumeStartupPrefetch } = await importFreshRegistry();
    const originalDebug = console.debug;

    try {
        console.debug = () => {};

        startStartupPrefetch('demo', async () => {
            throw new Error('prefetch failed');
        });

        assert.equal(await consumeStartupPrefetch('demo', async () => 'loaded'), 'loaded');
    } finally {
        console.debug = originalDebug;
    }
});
