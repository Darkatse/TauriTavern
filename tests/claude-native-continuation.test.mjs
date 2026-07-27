import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import test from 'node:test';
import vm from 'node:vm';

import { appendClaudeRefusalWarning, ClaudeNativeStreamAccumulator, getClaudeStopStatus, hasClaudeToolUse } from '../src/scripts/tauritavern/claude-native-stream.js';

async function smoothEvents(events) {
    const source = await readFile(new URL('../src/scripts/sse-stream.js', import.meta.url), 'utf8');
    const executableSource = source
        .replace(/^import .*;$/gm, '')
        .replace(/^export default .*;$/gm, '')
        .replace(/\bexport (?=(?:class|function)\b)/g, '');
    const context = vm.createContext({
        JSON,
        MessageEvent,
        TextDecoderStream,
        TransformStream,
        console: { debug() {} },
        document: { hasFocus: () => false },
        power_user: { smooth_streaming_no_think: false, smooth_streaming_speed: 50 },
        structuredClone,
        delay: async () => {},
    });
    vm.runInContext(`${executableSource}\nglobalThis.SmoothEventSourceStream = SmoothEventSourceStream;`, context);

    const stream = new context.SmoothEventSourceStream();
    const output = (async () => {
        const reader = stream.readable.getReader();
        const values = [];
        for (;;) {
            const { done, value } = await reader.read();
            if (done) return values;
            values.push(JSON.parse(value.data));
        }
    })();
    const writer = stream.writable.getWriter();
    const frames = events.map(event => `data: ${JSON.stringify(event)}\n\n`).join('');
    await writer.write(new TextEncoder().encode(frames));
    await writer.close();
    return output;
}

test('Claude native blocks survive smooth streaming and retain tool JSON', async () => {
    const events = await smoothEvents([
        { type: 'content_block_start', index: 0, content_block: { type: 'thinking', thinking: '', signature: '' } },
        { type: 'content_block_delta', index: 0, delta: { type: 'thinking_delta', thinking: 'Plan' } },
        { type: 'content_block_delta', index: 0, delta: { type: 'signature_delta', signature: 'opaque' } },
        { type: 'content_block_stop', index: 0 },
        { type: 'content_block_start', index: 1, content_block: { type: 'redacted_thinking', data: 'redacted' } },
        { type: 'content_block_stop', index: 1 },
        { type: 'content_block_start', index: 2, content_block: { type: 'text', text: '' } },
        { type: 'content_block_delta', index: 2, delta: { type: 'text_delta', text: 'Checking' } },
        { type: 'content_block_stop', index: 2 },
        { type: 'content_block_start', index: 3, content_block: { type: 'tool_use', id: 'call_1', name: 'weather', input: {} } },
        { type: 'content_block_delta', index: 3, delta: { type: 'input_json_delta', partial_json: '{"city":' } },
        { type: 'content_block_delta', index: 3, delta: { type: 'input_json_delta', partial_json: '"Paris"}' } },
        { type: 'content_block_stop', index: 3 },
        { type: 'message_delta', delta: { stop_reason: 'tool_use', stop_details: null } },
        { type: 'message_stop' },
    ]);
    const accumulator = new ClaudeNativeStreamAccumulator();
    let native = null;
    for (const event of events) {
        native = accumulator.consume(event) ?? native;
    }

    assert.deepEqual(native, {
        claude: {
            content: [
                { type: 'thinking', thinking: 'Plan', signature: 'opaque' },
                { type: 'redacted_thinking', data: 'redacted' },
                { type: 'text', text: 'Checking' },
                { type: 'tool_use', id: 'call_1', name: 'weather', input: { city: 'Paris' } },
            ],
            stop_reason: 'tool_use',
        },
    });
    assert.equal(events.find(event => event.delta?.thinking)?.delta.type, 'thinking_delta');
    assert.equal(events.find(event => event.delta?.text)?.delta.type, 'text_delta');
    assert.equal(hasClaudeToolUse(native), true);
    assert.equal(accumulator.finish(), native);
});

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

test('Legacy prompt replay enables native metadata for Claude Messages transports', async () => {
    const source = await readFile(new URL('../src/scripts/openai.js', import.meta.url), 'utf8');
    assert.match(source, /case chat_completion_sources\.CLAUDE:\s*return true/);
    assert.match(source, /case chat_completion_sources\.CUSTOM:\s*return settings\.custom_api_format === custom_api_formats\.CLAUDE_MESSAGES/);
    assert.match(source, /case chat_completion_sources\.VERTEXAI:\s*return isVertexAiClaudeModelId\(model\)/);
    assert.match(source, /case chat_completion_sources\.AWS_BEDROCK:[\s\S]*?anthropic\\\.claude/);
    assert.equal(source.match(/const includeClaudeNative = usesClaudeMessagesSemantics/g)?.length, 2);
    assert.match(source, /new ClaudeNativeStreamAccumulator\(\)/);
    assert.match(source, /const stopStatus = claudeNative && nativeDelta[\s\S]*?getClaudeStopStatus/);
    assert.match(source, /toolCalls\.length = 0/);
    assert.match(source, /delete message\.tool_calls/);
    assert.match(source, /hasClaudeToolUse\(nativeDelta\)/);
    assert.match(source, /!hasClaudeToolUse\(message\.native\)/);
});

test('Legacy refusal keeps provider output and appends a visible warning', async () => {
    const [scriptSource, openaiSource] = await Promise.all([
        readFile(new URL('../src/script.js', import.meta.url), 'utf8'),
        readFile(new URL('../src/scripts/openai.js', import.meta.url), 'utf8'),
    ]);
    assert.match(openaiSource, /const stopStatus = claudeNative && nativeDelta[\s\S]*?toolCalls\.length = 0[\s\S]*?toastr\.error[\s\S]*?text = appendClaudeRefusalWarning/);
    assert.match(openaiSource, /stopStatus\?\.code === 'model\.provider_refusal'[\s\S]*?message\.content = appendClaudeRefusalWarning/);
    assert.doesNotMatch(scriptSource, /appendClaudeRefusalWarning|initialChatLength|initialMessage|initialTextareaValue|discardOutput/);
});

test('Editing a split assistant turn invalidates both native copies', async () => {
    const source = await readFile(new URL('../src/script.js', import.meta.url), 'utf8');
    assert.match(
        source,
        /if \(mes\.mes !== text\) \{[\s\S]*?delete mes\.extra\.native;[\s\S]*?pairedMessageId[\s\S]*?delete pairedMessage\?\.extra\?\.native;/,
    );
});
