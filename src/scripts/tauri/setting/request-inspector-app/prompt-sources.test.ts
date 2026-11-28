import { expect, test } from '@rstest/core';

import type { PromptSource, RequestView } from './RequestInspectorContract';
import { buildCurl } from './curl';
import { SYSTEM_MESSAGE, locateSources, textPartKey } from './prompt-sources';

const text = (value: string) => ({ type: 'text' as const, text: value, cacheBreakpoint: false });

function source(role: string, pieces: Partial<PromptSource['pieces'][number]>[], extra: Partial<PromptSource> = {}): PromptSource {
    return {
        role,
        tokens: 1,
        pieces: pieces.map(piece => ({ type: 'prompt', text: '', tokens: 1, ...piece })),
        ...extra,
    };
}

test('sources are located through wire conversion: merged system, name prefix, same-role merge, tool turns', () => {
    const sources = [
        source('system', [{ identifier: 'main', text: 'You are a narrator.' }]),
        source('system', [{ identifier: 'worldInfoBefore', text: '  Keep the lamp lit.\n' }]),
        source('user', [{ type: 'chat', messageId: 1, text: 'Hello there' }]),
        // One injected message assembled from two entries at the same depth.
        source('user', [
            { identifier: 'nudge', depth: 0, text: 'Stay calm.' },
            { type: 'extensionPrompt', identifier: '2_floating_prompt', depth: 0, text: 'Be brief.' },
        ]),
        source('assistant', [{ type: 'chat', messageId: 2 }], { toolCallIds: ['call_1'] }),
        source('tool', [{ type: 'chat', text: '{"ok":true}' }], { toolCallId: 'call_1' }),
        source('system', [{ identifier: 'rewritten', text: 'An extension rewrote this before the send.' }]),
        source('tool', [{ type: 'chat', text: 'gone' }], { toolCallId: 'call_9' }),
    ];
    const merged = 'Alice: Hello there\n\nStay calm.\nBe brief.';
    const request: RequestView = {
        kind: 'chat',
        systemField: 'system',
        system: [text('You are a narrator.\nKeep the lamp lit.')],
        messages: [
            { role: 'user', parts: [text(merged)] },
            { role: 'assistant', parts: [{ type: 'toolCall', id: 'call_1', name: 'read_state', arguments: {} }] },
            { role: 'user', parts: [{ type: 'toolResult', callId: 'call_1', parts: [text('{"ok":true}')] }] },
        ],
        tools: [],
        parameters: [],
    };

    const located = locateSources(request, sources);

    expect(located.segments.get(textPartKey(SYSTEM_MESSAGE, 0))).toEqual([
        { source: 0, piece: 0, start: 0 },
        { source: 1, piece: 0, start: 20 },
    ]);
    expect(located.segments.get(textPartKey(0, 0))).toEqual([
        { source: 2, piece: 0, start: merged.indexOf('Hello') },
        { source: 3, piece: 0, start: merged.indexOf('Stay') },
        { source: 3, piece: 1, start: merged.indexOf('Be brief') },
    ]);
    expect([...located.calls]).toEqual([['call_1', 4]]);
    expect([...located.results]).toEqual([['call_1', 5]]);
    expect(located.unlocated).toEqual([{ source: 6, piece: 0 }, { source: 7, piece: 0 }]);
});

test('cURL names credentials as variables, survives quotes, and ends its heredoc after the body', () => {
    const body = '{\n  "note": "it\'s",\nTT_REQUEST_BODY\n}';
    const curl = buildCurl({
        transport: 'http',
        method: 'POST',
        url: 'https://api.example.com/v1/models/x:generate?key=[redacted]&alt=sse',
        headers: [
            { name: 'authorization', value: '[redacted]' },
            { name: 'x-title', value: 'Bob\'s "app"' },
        ],
    }, body);

    expect(curl).toBe([
        'curl -X POST "https://api.example.com/v1/models/x:generate?key=$QUERY_KEY&alt=sse" \\',
        '  -H "authorization: $HEADER_AUTHORIZATION" \\',
        '  -H \'x-title: Bob\'\\\'\'s "app"\' \\',
        '  --data-binary @- <<\'TT_REQUEST_BODY_1\'',
        body,
        'TT_REQUEST_BODY_1',
        '',
    ].join('\n'));
    expect(buildCurl({ transport: 'websocket', method: 'GET', url: 'wss://api.example.com/v1/responses', headers: [] }, body)).toBeNull();
});
