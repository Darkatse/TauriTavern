import test from 'node:test';
import assert from 'node:assert/strict';
import path from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';

const REPO_ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');

async function importFresh(modulePath) {
    const url = `${pathToFileURL(modulePath).href}?t=${Date.now()}-${Math.random()}`;
    return import(url);
}

function nextMicrotask() {
    return Promise.resolve();
}

function nextTick() {
    return new Promise((resolve) => {
        setImmediate(resolve);
    });
}

test('generation idle gate blocks until markIdle()', async () => {
    const { createGenerationIdleGate } = await importFresh(
        path.join(REPO_ROOT, 'src/scripts/util/generation-idle-gate.js'),
    );

    const gate = createGenerationIdleGate();
    await gate.wait();

    gate.markBusy();
    const pending = gate.wait();

    let resolved = false;
    pending.then(() => {
        resolved = true;
    });

    await nextMicrotask();
    assert.equal(resolved, false);

    gate.markBusy();
    assert.equal(gate.wait(), pending);

    gate.markIdle();
    await pending;
    assert.equal(resolved, true);

    gate.markIdle();
    await gate.wait();
});

test('safe generate waits for idle and serializes callers', async () => {
    const { createGenerationIdleGate } = await importFresh(
        path.join(REPO_ROOT, 'src/scripts/util/generation-idle-gate.js'),
    );
    const { createSafeGenerate } = await importFresh(
        path.join(REPO_ROOT, 'src/scripts/util/safe-generate.js'),
    );

    const gate = createGenerationIdleGate();
    gate.markBusy();

    const calls = [];
    const generate = async (...args) => {
        calls.push(args);
        return args.join(':');
    };

    const safeGenerate = createSafeGenerate({
        waitForIdle: gate.wait,
        generate,
    });

    const run = safeGenerate('regenerate');
    await nextMicrotask();
    assert.equal(calls.length, 0);

    gate.markIdle();
    const result = await run;
    assert.deepEqual(calls, [['regenerate']]);
    assert.equal(result, 'regenerate');

    const started = [];
    const resolvers = [];
    const slowGenerate = (label) => {
        started.push(label);
        return new Promise((resolve) => {
            resolvers.push(resolve);
        });
    };

    const safeSlowGenerate = createSafeGenerate({
        waitForIdle: gate.wait,
        generate: slowGenerate,
    });

    const first = safeSlowGenerate('first');
    await nextTick();
    assert.deepEqual(started, ['first']);

    const second = safeSlowGenerate('second');
    await nextTick();
    assert.deepEqual(started, ['first']);

    resolvers[0]('ok-1');
    await nextTick();
    assert.deepEqual(started, ['first', 'second']);

    resolvers[1]('ok-2');
    assert.equal(await first, 'ok-1');
    assert.equal(await second, 'ok-2');
});

test('safe generate does not stall after a rejected run', async () => {
    const { createSafeGenerate } = await importFresh(
        path.join(REPO_ROOT, 'src/scripts/util/safe-generate.js'),
    );

    let callCount = 0;
    const generate = async () => {
        callCount += 1;
        if (callCount === 1) {
            throw new Error('boom');
        }
        return 'ok';
    };

    const safeGenerate = createSafeGenerate({
        waitForIdle: () => Promise.resolve(),
        generate,
    });

    await assert.rejects(() => safeGenerate('first'), /boom/);
    assert.equal(await safeGenerate('second'), 'ok');
});
