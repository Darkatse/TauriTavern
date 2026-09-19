import assert from 'node:assert/strict';
import test from 'node:test';
import { parseParams } from '../src/scripts/tauri/generation-params/json-view.js';

const schema = new Map([
    ['temperature', { type: 'number', min: 0, max: 2 }],
    ['top_k', { type: 'number', min: 0 }],
    ['reasoning_effort', { type: 'string', options: ['auto', 'low', 'high'] }],
    ['function_calling', { type: 'boolean' }],
]);

test('unknown keys and out-of-range values are reported, valid keys still returned', () => {
    const { values, errors } = parseParams(
        '{"temperature": 5, "messages": [], "top_k": 3, "reasoning_effort": "max", "function_calling": "yes"}',
        schema,
    );
    assert.deepEqual([...values], [['top_k', 3]]);
    assert.deepEqual(errors, [
        { kind: 'invalid', key: 'temperature' },
        { kind: 'unknown', key: 'messages' },
        { kind: 'invalid', key: 'reasoning_effort' },
        { kind: 'invalid', key: 'function_calling' },
    ]);
});

test('malformed input is a syntax error, not a partial apply', () => {
    for (const text of ['{', '[]', '42', 'null']) {
        const { values, errors } = parseParams(text, schema);
        assert.equal(values.size, 0);
        assert.equal(errors.length, 1);
        assert.equal(errors[0].kind, 'syntax');
    }
});
