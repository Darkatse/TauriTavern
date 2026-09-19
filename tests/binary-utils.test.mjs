import assert from 'node:assert/strict';
import test from 'node:test';

import { encodeBytesToBase64 } from '../src/tauri/main/binary-utils.js';

for (const legacy of [false, true]) {
    test('base64 preserves padding and byte views' + (legacy ? ' without the native API' : ''), t => {
        t.diagnostic('Native toBase64 available: ' + (typeof Uint8Array.prototype.toBase64 === 'function'));
        for (const length of [0, 1, 2, 3, 0x7fff, 0x8000, 0x8001]) {
            const buffer = Uint8Array.from({ length: length + 2 }, (_, i) => i % 256);
            const view = buffer.subarray(1, length + 1);
            if (legacy) {
                Object.defineProperty(view, 'toBase64', { value: undefined });
            }
            assert.equal(encodeBytesToBase64(view), Buffer.from(view).toString('base64'));
        }
    });
}
