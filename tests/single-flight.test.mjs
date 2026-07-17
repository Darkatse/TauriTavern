import assert from 'node:assert/strict';
import test from 'node:test';

import { createSingleFlight } from '../src/scripts/util/single-flight.js';

function deferred() {
    let resolve;
    const promise = new Promise(resolvePromise => {
        resolve = resolvePromise;
    });
    return { promise, resolve };
}

test('identical concurrent keys share one task and result', async () => {
    const gate = deferred();
    const singleFlight = createSingleFlight();
    let calls = 0;
    const task = () => {
        calls += 1;
        return gate.promise;
    };

    const first = singleFlight('same', task);
    const second = singleFlight('same', task);
    await Promise.resolve();
    assert.equal(calls, 1);

    gate.resolve([1, 2, 3]);
    assert.strictEqual(await first, await second);
});

test('different keys run independently', async () => {
    const singleFlight = createSingleFlight();
    const values = await Promise.all([
        singleFlight('first', async () => 1),
        singleFlight('second', async () => 2),
    ]);

    assert.deepEqual(values, [1, 2]);
});

test('settled success and failure entries are removed', async () => {
    const singleFlight = createSingleFlight();
    let calls = 0;

    assert.equal(await singleFlight('success', async () => ++calls), 1);
    await Promise.resolve();
    assert.equal(await singleFlight('success', async () => ++calls), 2);

    await assert.rejects(singleFlight('failure', async () => {
        calls += 1;
        throw new Error('expected failure');
    }), /expected failure/);
    await Promise.resolve();
    assert.equal(await singleFlight('failure', async () => ++calls), 4);
});
