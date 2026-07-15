import test from 'node:test';
import assert from 'node:assert/strict';

import {
    getMessageRenderBatches,
} from '../src/scripts/tauri/perf/message-render-batches.js';

test('message prepend rendering is split into stable contiguous batches', () => {
    assert.deepEqual(getMessageRenderBatches(12, 5), [
        { start: 0, end: 5 },
        { start: 5, end: 10 },
        { start: 10, end: 12 },
    ]);
    assert.deepEqual(getMessageRenderBatches(0), []);
});
