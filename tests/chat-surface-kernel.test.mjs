import test from 'node:test';
import assert from 'node:assert/strict';

import { createChatProjection } from '../src/tauri/main/kernel/chat-surface/projection.js';
import { createStructureSnapshotFactory } from '../src/tauri/main/kernel/chat-surface/structure-snapshot.js';
import { createBoundedProjectionLayout } from '../src/tauri/main/kernel/chat-surface/projection-layout.js';

test('ChatSurface structure keys follow message objects and reset with each epoch', () => {
    const first = { mes: 'first' };
    const second = { mes: 'second' };
    const structures = createStructureSnapshotFactory();

    const initial = structures.beginEpoch([first, second]);
    first.mes = 'streamed content';
    assert.equal(structures.update([first, second]), initial);

    const reordered = structures.update([second, first]);
    assert.deepEqual(reordered.keys, [initial.keys[1], initial.keys[0]]);

    const replacement = structures.update([second, { mes: 'replacement' }]);
    assert.equal(replacement.keys[0], initial.keys[1]);
    assert.notEqual(replacement.keys[1], initial.keys[0]);

    const nextEpoch = structures.beginEpoch([first]);
    assert.notEqual(nextEpoch.keys[0], initial.keys[0]);
    assert.throws(() => structures.update([first, first]), /same object more than once/);
});

test('ChatSurface projection validates exact one-or-two-range intent', () => {
    const projection = createChatProjection([0, 1, 7, 8], { count: 10 });
    assert.deepEqual(projection.indices, [0, 1, 7, 8]);
    assert.deepEqual(projection.ranges, [{ start: 0, end: 2 }, { start: 7, end: 9 }]);
    assert.ok(Object.isFrozen(projection));
    assert.ok(Object.isFrozen(projection.indices));

    assert.throws(() => createChatProjection([1, 1], { count: 2 }), /strictly increasing/);
    assert.throws(() => createChatProjection([2], { count: 2 }), /outside/);
    assert.throws(() => createChatProjection([0, 2, 4], { count: 5 }), /maximum is 2/);
});

test('bounded projection layout derives exact top and middle spacer geometry', () => {
    const viewportItems = [
        { index: 10, start: 2020, end: 2220 },
        { index: 11, start: 2230, end: 2430 },
    ];
    const tail = { index: 99, start: 20000, end: 20200 };
    const layout = createBoundedProjectionLayout({
        count: 100,
        viewportItems,
        projectedItems: [...viewportItems, tail],
        paddingStart: 10,
        gap: 10,
        maxViewportItems: 32,
    });

    assert.deepEqual(layout.projection.indices, [10, 11, 99]);
    assert.deepEqual(layout.projection.ranges, [{ start: 10, end: 12 }, { start: 99, end: 100 }]);
    assert.deepEqual(layout.topSpacer, { present: true, height: 2000 });
    assert.deepEqual(layout.middleSpacer, { present: true, height: 17550 });
    assert.equal(layout.tailMessageId, 99);
});

test('bounded projection layout supports tail-only bootstrap and rejects dishonest geometry', () => {
    const bootstrap = createBoundedProjectionLayout({
        count: 10,
        viewportItems: [],
        projectedItems: [{ index: 9, start: 900, end: 1000 }],
        paddingStart: 0,
        gap: 10,
        maxViewportItems: 32,
    });
    assert.deepEqual(bootstrap.projection.indices, [9]);
    assert.deepEqual(bootstrap.topSpacer, { present: true, height: 890 });
    assert.equal(bootstrap.middleSpacer.present, false);

    assert.throws(() => createBoundedProjectionLayout({
        count: 10,
        viewportItems: [{ index: 1, start: 100, end: 200 }, { index: 3, start: 300, end: 400 }],
        projectedItems: [
            { index: 1, start: 100, end: 200 },
            { index: 3, start: 300, end: 400 },
            { index: 9, start: 900, end: 1000 },
        ],
        maxViewportItems: 32,
    }), /contiguous/);
    assert.throws(() => createBoundedProjectionLayout({
        count: 10,
        viewportItems: [{ index: 8, start: 900, end: 950 }],
        projectedItems: [{ index: 8, start: 900, end: 950 }, { index: 9, start: 940, end: 1000 }],
        maxViewportItems: 32,
    }), /overlapping/);
});
