import test from 'node:test';
import assert from 'node:assert/strict';

import { projectToolTurns } from '../src/scripts/tauritavern/tool-turn-projection.js';

const call = (id = 'call-1', overrides = {}) => ({
    id,
    name: 'lookup',
    parameters: '{"query":"weather"}',
    ...overrides,
});

const assistant = (calls, overrides = {}) => ({
    is_user: false,
    is_system: false,
    mes: '',
    ...(calls === undefined ? {} : { tool_calls: calls }),
    ...overrides,
});

const tool = (id = 'call-1', overrides = {}) => ({
    role: 'tool',
    is_user: false,
    is_system: true,
    name: 'lookup',
    tool_call_id: id,
    mes: '{"temperature":21}',
    ...overrides,
});

const legacyFloor = (invocations, overrides = {}) => ({
    is_user: false,
    is_system: true,
    mes: '<details>legacy tool floor</details>',
    extra: { tool_invocations: invocations },
    ...overrides,
});

test('ordinary chat passes through by identity', () => {
    const user = { is_user: true, is_system: false, mes: 'Hello' };
    const reply = assistant(undefined, { mes: 'Hi' });
    const projection = projectToolTurns([user, reply]);

    assert.deepEqual(projection, [
        { type: 'message', sourceIndex: 0, message: user },
        { type: 'message', sourceIndex: 1, message: reply },
    ]);
});

test('empty Assistant owns parallel first-class Tool results by call ID', () => {
    const firstCall = call('call-1', { displayName: 'Weather lookup', signature: null });
    const owner = assistant([firstCall, call('call-2', { name: 'clock', parameters: '{}' })]);
    const firstResult = tool('call-1');
    const secondResult = tool('call-2', { name: 'clock', mes: '', error: true });
    const projection = projectToolTurns([owner, firstResult, secondResult]);

    assert.equal(projection.length, 1);
    assert.equal(projection[0].assistantMessage, owner);
    assert.deepEqual(projection[0].sourceIndices, [0, 1, 2]);
    assert.deepEqual(projection[0].invocations, [
        { ...firstCall, result: firstResult.mes },
        { ...call('call-2', { name: 'clock', parameters: '{}' }), result: '', error: true },
    ]);
});

test('Tool results may be physically separated from their Assistant by side-effect messages', () => {
    const owner = assistant([call()]);
    const sideEffect = assistant(undefined, { mes: 'Generated an image', extra: { media: [{ url: 'image.png' }] } });
    const result = tool();
    const finalReply = assistant(undefined, { mes: 'Done' });
    const projection = projectToolTurns([owner, sideEffect, result, finalReply]);

    assert.deepEqual(projection.map(entry => entry.type), ['tool-turn', 'message', 'message']);
    assert.deepEqual(projection[0].sourceIndices, [0, 2]);
    assert.equal(projection[1].message, sideEffect);
    assert.equal(projection[2].message, finalReply);
});

test('recursive first-class tool rounds retain model-turn order', () => {
    const firstOwner = assistant([call('call-1')]);
    const secondOwner = assistant([call('call-2')], { mes: 'One more check' });
    const finalReply = assistant(undefined, { mes: 'Done' });
    const projection = projectToolTurns([
        firstOwner,
        tool('call-1'),
        secondOwner,
        tool('call-2'),
        finalReply,
    ]);

    assert.deepEqual(projection.map(entry => entry.type), ['tool-turn', 'tool-turn', 'message']);
    assert.deepEqual(projection.slice(0, 2).map(entry => entry.invocations[0].id), ['call-1', 'call-2']);
});

test('old tool turns can be stripped while pure Assistant history and the active chain remain', () => {
    const oldOwner = assistant([call('old-call')], { mes: 'Tool-bearing text is omitted' });
    const pureAssistant = assistant(undefined, { mes: 'Pure roleplay remains' });
    const user = { is_user: true, is_system: false, mes: 'Continue' };
    const currentOwner = assistant([call('current-call')]);
    const projection = projectToolTurns([
        oldOwner,
        tool('old-call'),
        pureAssistant,
        user,
        currentOwner,
        tool('current-call'),
    ], true);

    assert.deepEqual(projection.map(entry => entry.type), ['message', 'message', 'tool-turn']);
    assert.equal(projection[0].message, pureAssistant);
    assert.equal(projection[1].message, user);
    assert.equal(projection[2].assistantMessage, currentOwner);
});

test('adjacent legacy system floor is read-only projected into the preceding Assistant', () => {
    const owner = assistant(undefined, { mes: 'I will check.' });
    const invocation = { ...call(), result: 'sunny', error: false };
    const floor = legacyFloor([invocation]);
    const projection = projectToolTurns([owner, floor]);

    assert.equal(projection.length, 1);
    assert.equal(projection[0].assistantMessage, owner);
    assert.equal(projection[0].metadataMessage, floor);
    assert.deepEqual(projection[0].invocations, [invocation]);
});

test('standalone legacy floor synthesizes an empty provider Assistant', () => {
    const floor = legacyFloor([{ ...call(), result: 'sunny' }]);
    const followingAssistant = assistant(undefined, { mes: 'Final response' });
    const chat = [floor, followingAssistant];
    const projection = projectToolTurns(chat);

    assert.equal(projection[0].assistantMessage, null);
    assert.equal(projection[0].metadataMessage, floor);
    assert.equal(projection[1].message, followingAssistant);
});

test('legacy JSON values normalize without mutating stored history', () => {
    const rawInvocation = { ...call(), parameters: { query: 'weather' }, result: { temperature: 21 } };
    const floor = legacyFloor([rawInvocation]);
    const projection = projectToolTurns([floor]);

    assert.equal(projection[0].invocations[0].parameters, '{"query":"weather"}');
    assert.equal(projection[0].invocations[0].result, '{"temperature":21}');
    assert.deepEqual(rawInvocation.parameters, { query: 'weather' });
    assert.deepEqual(rawInvocation.result, { temperature: 21 });
});

test('empty containers and non-protocol metadata do not block replay', () => {
    const emptyCalls = assistant([], {
        role: 'assistant',
        tool_call_id: 'stale-id',
        mes: 'No tools after all',
    });
    const emptyLegacy = legacyFloor([]);
    const owner = assistant([call('call-1', { displayName: 42, signature: {} })], {
        extra: { tool_invocations: [] },
    });
    const result = tool('call-1', {
        is_user: true,
        is_system: false,
        name: undefined,
        error: 'failed',
        tool_calls: [],
        extra: { tool_invocations: [] },
    });
    const projection = projectToolTurns([emptyCalls, emptyLegacy, owner, result]);

    assert.deepEqual(projection.slice(0, 2), [
        { type: 'message', sourceIndex: 0, message: emptyCalls },
        { type: 'message', sourceIndex: 1, message: emptyLegacy },
    ]);
    assert.deepEqual(projection[2].invocations, [{
        id: 'call-1',
        name: 'lookup',
        parameters: '{"query":"weather"}',
        result: '{"temperature":21}',
    }]);
});

test('canonical malformed, orphan, duplicate, and missing relations fail at the first exact chat path', () => {
    const cases = [
        {
            chat: [assistant([call()], { is_system: true }), tool()],
            expected: 'chat[0].tool_calls is only valid on an Assistant message',
        },
        {
            chat: [assistant([call()]), tool('call-1', { tool_calls: [call('nested')] })],
            expected: 'chat[1].tool_calls is only valid on an Assistant message',
        },
        {
            chat: [assistant([call()])],
            expected: 'chat[0].tool_calls[0] has no matching Tool result',
        },
        {
            chat: [assistant([call()]), tool(), tool()],
            expected: 'chat[2].tool_call_id duplicates an earlier Tool result',
        },
        {
            chat: [assistant([call()]), tool('other-call')],
            expected: 'chat[1].tool_call_id does not match any preceding Assistant tool call',
        },
        {
            chat: [tool(), assistant([call()])],
            expected: 'chat[0].tool_call_id does not match any preceding Assistant tool call',
        },
        {
            chat: [assistant([call(), call()]), tool()],
            expected: 'chat[0].tool_calls[1].id duplicates an earlier tool call id',
        },
        {
            chat: [{ is_user: true, is_system: false, mes: 'bad', tool_calls: [call()] }],
            expected: 'chat[0].tool_calls is only valid on an Assistant message',
        },
    ];

    for (const { chat, expected } of cases) {
        assert.throws(() => projectToolTurns(chat), error => error.message.includes(expected));
    }
});

test('IDs may be reused by completed later turns but not by overlapping pending turns', () => {
    const first = assistant([call('same')]);
    const second = assistant([call('same')]);
    const repeatedAcrossTurns = projectToolTurns([first, tool('same'), second, tool('same')]);
    assert.deepEqual(repeatedAcrossTurns.map(entry => entry.invocations[0].id), ['same', 'same']);

    assert.throws(() => projectToolTurns([
        assistant([call('pending')]),
        assistant([call('pending')]),
        tool('pending'),
        tool('pending'),
    ]), /matches multiple unresolved Assistant tool calls/);
});

test('mixed canonical and legacy facts fail instead of choosing one silently', () => {
    const owner = assistant([call()], { extra: { tool_invocations: [{ ...call(), result: 'legacy' }] } });
    assert.throws(
        () => projectToolTurns([owner, tool()]),
        /cannot contain both tool_calls and extra\.tool_invocations/,
    );

    const invalidLegacyCarrier = assistant(undefined, {
        extra: { tool_invocations: [{ ...call(), result: 'legacy' }] },
    });
    assert.throws(
        () => projectToolTurns([invalidLegacyCarrier]),
        /only valid on a legacy system tool floor/,
    );
});
