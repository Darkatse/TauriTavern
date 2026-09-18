import test from 'node:test';
import assert from 'node:assert/strict';
import { createDbApi } from '../src/tauri/main/api/db.js';

test('database handles become available only after native open, and opening errors propagate', async () => {
    const opened = Promise.withResolvers();
    const api = createDbApi({ safeInvoke: () => opened.promise });
    let delivered = false;
    const opening = api.open('memory', { dim: 2, syncMode: 'full' }).then(handle => {
        delivered = true;
        return handle;
    });
    await Promise.resolve();
    assert.equal(delivered, false);
    opened.resolve({ namespace: 'memory', dim: 2, options: { dim: 2, syncMode: 'full' } });
    await opening;
    assert.equal(delivered, true);

    const failed = createDbApi({ safeInvoke: async () => { throw new Error('disk unavailable'); } });
    await assert.rejects(failed.open('memory'), /disk unavailable/);
});

test('text search separates query text from config and translates the filter alias', async () => {
    const requests = [];
    const api = createDbApi({ safeInvoke: async (_command, { request }) => {
        requests.push(structuredClone(request));
        return request.type === 'open' ? { namespace: 'memory', dim: 2, options: { dim: 2 } } : null;
    } });
    const db = await api.open('memory', { dim: 2 });
    await db.search(null, { queryText: 'memory', filter: { kind: 'fact' } });
    assert.deepEqual(requests[1].operation, {
        type: 'search', vector: null, queryText: 'memory', config: { payloadFilter: { kind: 'fact' } },
    });
});
