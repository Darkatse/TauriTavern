import assert from 'node:assert/strict';
import test from 'node:test';

import { appendClaudeRefusalWarning, ClaudeNativeStreamAccumulator, getClaudeStopStatus, hasClaudeToolUse } from '../src/scripts/tauritavern/claude-native-stream.js';

test('Claude input JSON deltas follow the delta contract, not the block type', () => {
    const accumulator = new ClaudeNativeStreamAccumulator();
    accumulator.consume({ type: 'content_block_start', index: 0, content_block: { type: 'server_tool_use', id: 'srvtoolu_1', name: 'web_search', input: {} } });
    accumulator.consume({ type: 'content_block_delta', index: 0, delta: { type: 'input_json_delta', partial_json: '{"query":"weather"}' } });
    accumulator.consume({ type: 'content_block_stop', index: 0 });
    const native = accumulator.consume({ type: 'message_stop' });

    assert.deepEqual(native?.claude?.content[0]?.input, { query: 'weather' });
    assert.equal(hasClaudeToolUse(native), false);
});
test('Claude native accumulator rejects incomplete tool JSON', () => {
    const accumulator = new ClaudeNativeStreamAccumulator();
    accumulator.consume({ type: 'content_block_start', index: 0, content_block: { type: 'tool_use', id: 'call_1', name: 'weather', input: {} } });
    accumulator.consume({ type: 'content_block_delta', index: 0, delta: { type: 'input_json_delta', partial_json: '{' } });
    assert.throws(
        () => accumulator.consume({ type: 'content_block_stop', index: 0 }),
        /Claude tool_use block contains invalid JSON/,
    );
});

test('Claude terminal status distinguishes refusal and truncation', () => {
    assert.deepEqual(
        getClaudeStopStatus('refusal', { explanation: 'This request was declined.' }),
        { code: 'model.provider_refusal', message: 'This request was declined.' },
    );
    assert.equal(getClaudeStopStatus('max_tokens')?.code, 'model.output_truncated');
    assert.equal(getClaudeStopStatus('model_context_window_exceeded')?.code, 'model.output_truncated');
    assert.equal(getClaudeStopStatus('end_turn'), null);
});

test('Claude refusal warning preserves provider output', () => {
    assert.equal(
        appendClaudeRefusalWarning('Provider output.', 'Request declined.'),
        'Provider output.\n\n⚠️ Request declined.',
    );
    assert.equal(
        appendClaudeRefusalWarning('', 'Request declined.'),
        '⚠️ Request declined.',
    );
});
