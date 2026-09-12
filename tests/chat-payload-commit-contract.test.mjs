import assert from 'node:assert/strict';
import test from 'node:test';

import { commitChatPayload } from '../src/scripts/tauri/chat/commit.js';
import {
    saveCharacterChatPayload,
    saveGroupChatPayload,
} from '../src/scripts/chat-payload-transport.js';
import { jsonResponse } from '../src/tauri/main/http-utils.js';
import { stripJsonl } from '../src/tauri/main/kernel/chat-utils.js';
import { createRouteRegistry } from '../src/tauri/main/router.js';
import { registerChatRoutes } from '../src/tauri/main/routes/chat-routes.js';

const SESSION_ID = '10000000-0000-4000-8000-000000000001';
const TARGET = Object.freeze({
    kind: 'character',
    characterId: 'Alice',
    fileName: 'Story',
});

function installRuntime(userAgent, invoke) {
    const previousWindow = globalThis.window;
    const previousNavigator = Object.getOwnPropertyDescriptor(globalThis, 'navigator');

    globalThis.window = { __TAURI__: { core: { invoke } } };
    Object.defineProperty(globalThis, 'navigator', {
        value: { userAgent },
        configurable: true,
    });

    return () => {
        if (previousWindow === undefined) {
            delete globalThis.window;
        } else {
            globalThis.window = previousWindow;
        }

        if (previousNavigator) {
            Object.defineProperty(globalThis, 'navigator', previousNavigator);
        } else {
            delete globalThis.navigator;
        }
    };
}

function createCommitHost({
    maxFrameBytes = 4,
    onAppend,
    finishSizeDelta = 0,
    finishError,
    abortError,
    expectedTarget = TARGET,
    expectedForce = false,
    expectedCommitReason = 'mutation',
} = {}) {
    const calls = [];
    const frames = [];

    return {
        calls,
        frames,
        invoke: async (command, args, options) => {
            calls.push({ command, args, options });

            if (command === 'begin_chat_commit') {
                assert.deepEqual(args, { target: expectedTarget, force: expectedForce });
                return { sessionId: SESSION_ID, maxFrameBytes };
            }

            if (command === 'append_chat_commit_chunk') {
                assert.equal(options?.headers?.['session-id'], SESSION_ID);
                const offset = Number(options?.headers?.offset);
                const bytes = options?.headers?.['chunk-encoding'] === 'base64'
                    ? new Uint8Array(Buffer.from(args.data, 'base64'))
                    : args;
                frames.push({ offset, bytes });
                return onAppend
                    ? onAppend({ offset, bytes, index: frames.length - 1 })
                    : offset + bytes.byteLength;
            }

            if (command === 'finish_chat_commit') {
                assert.equal(args.sessionId, SESSION_ID);
                assert.equal(args.commitReason, expectedCommitReason);
                if (finishError) {
                    throw finishError;
                }
                return { size: args.expectedSize + finishSizeDelta };
            }

            if (command === 'abort_chat_commit') {
                assert.equal(args.sessionId, SESSION_ID);
                if (abortError) {
                    throw abortError;
                }
                return undefined;
            }

            throw new Error(`Unexpected command: ${command}`);
        },
    };
}

function commit(payload) {
    return commitChatPayload({
        target: TARGET,
        payload,
        force: false,
        commitReason: 'mutation',
    });
}

test('chat payload commit uses bounded Android base64 frames and exact offsets', async () => {
    const host = createCommitHost();
    const restore = installRuntime('Mozilla/5.0 (Linux; Android 14)', host.invoke);
    const payload = [{ user_name: 'A' }, { mes: '0123456789' }];

    try {
        await commit(payload);

        assert.ok(host.frames.length > 1);
        assert.ok(host.frames.every((frame) => frame.bytes.byteLength <= 4));
        assert.deepEqual(
            host.frames.map((frame) => frame.offset),
            host.frames.map((_, index, frames) => frames
                .slice(0, index)
                .reduce((total, frame) => total + frame.bytes.byteLength, 0)),
        );
        const appendCalls = host.calls.filter((call) => call.command === 'append_chat_commit_chunk');
        assert.ok(appendCalls.every((call) => typeof call.args.data === 'string'));
        assert.ok(appendCalls.every((call) => call.options.headers['chunk-encoding'] === 'base64'));
        assert.equal(
            Buffer.concat(host.frames.map((frame) => Buffer.from(frame.bytes))).toString(),
            payload.map(JSON.stringify).join('\n'),
        );
        assert.equal(host.calls.at(-1).command, 'finish_chat_commit');
    } finally {
        restore();
    }
});



for (const scenario of [
    {
        kind: 'character',
        target: TARGET,
        save: payload => saveCharacterChatPayload({
            characterName: 'Alice', avatarUrl: 'Alice.png', fileName: 'Story', payload,
        }),
    },
    {
        kind: 'group',
        target: { kind: 'group', chatId: 'Story' },
        save: payload => saveGroupChatPayload({ id: 'Story', payload }),
    },
]) {
    test(`${scenario.kind} save snapshots before yielding and keeps one frame in flight`, async () => {
        let releaseFirst;
        let markStarted;
        const started = new Promise((resolve) => { markStarted = resolve; });
        const host = createCommitHost({
            expectedTarget: scenario.target,
            onAppend: ({ offset, bytes, index }) => index === 0
                ? new Promise((resolve) => {
                    releaseFirst = () => resolve(offset + bytes.byteLength);
                    markStarted();
                })
                : offset + bytes.byteLength,
        });
        const restore = installRuntime('Mozilla/5.0 (Linux; Android 14)', host.invoke);
        const payload = [
            { chat_metadata: { integrity: 'chat', variables: { score: 1 } } },
            { mes: 'first message', extra: { extension: { enabled: true } } },
            { mes: 'last message', swipes: ['original swipe'] },
        ];
        const expected = payload.map(entry => JSON.stringify(entry)).join('\n');

        try {
            const pending = scenario.save(payload);
            payload[0].chat_metadata.variables.score = 2;
            payload[1].extra.extension.enabled = false;
            await started;
            assert.equal(host.frames.length, 1);
            payload[2].swipes[0] = 'changed while sending';
            payload.push({ mes: 'new message' });

            releaseFirst();
            await pending;
            assert.equal(
                Buffer.concat(host.frames.map(frame => Buffer.from(frame.bytes))).toString(),
                expected,
            );
        } finally {
            restore();
        }
    });
}

test('chat payload commit sends raw frames off Android', async () => {
    const host = createCommitHost();
    const restore = installRuntime('Mozilla/5.0 (Macintosh; Intel Mac OS X 14_0)', host.invoke);

    try {
        await commit([{ mes: '0123456789' }]);
        const appendCalls = host.calls.filter((call) => call.command === 'append_chat_commit_chunk');
        assert.ok(appendCalls.every((call) => call.args instanceof Uint8Array));
        assert.ok(appendCalls.every((call) => call.options.headers['chunk-encoding'] === undefined));
    } finally {
        restore();
    }
});

test('chat payload commit rejects invalid payloads before opening a session', async () => {
    const host = createCommitHost();
    const restore = installRuntime('Mozilla/5.0 (Macintosh)', host.invoke);

    try {
        await assert.rejects(() => commit([null]), /entry at index 0/i);
        assert.deepEqual(host.calls, []);
    } finally {
        restore();
    }
});

test('chat payload commit aborts ACK failures', async () => {
    const host = createCommitHost({
        onAppend: ({ offset, bytes }) => offset + bytes.byteLength + 1,
    });
    const restore = installRuntime('Mozilla/5.0 (Macintosh)', host.invoke);

    try {
        await assert.rejects(() => commit([{ mes: '012345' }]), /unexpected offset/i);
        assert.equal(host.calls.filter((call) => call.command === 'abort_chat_commit').length, 1);
    } finally {
        restore();
    }
});

test('chat payload commit preserves non-integrity finish errors', async () => {
    for (const finishError of [
        new Error('finish failed'),
        { InternalServerError: 'Could not read integrity metadata' },
        { BadRequest: 'Invalid chat header' },
        'integrity',
    ]) {
        const host = createCommitHost({ finishError });
        const restore = installRuntime('Mozilla/5.0 (Macintosh)', host.invoke);

        try {
            await assert.rejects(() => commit([{ mes: 'failure' }]), error => error === finishError);
            assert.equal(host.calls.filter(call => call.command === 'abort_chat_commit').length, 1);
        } finally {
            restore();
        }
    }
});

test('chat payload commit classifies integrity conflicts and aborts the session', async () => {
    const finishError = { BadRequest: 'integrity' };
    const host = createCommitHost({ finishError });
    const restore = installRuntime('Mozilla/5.0 (Macintosh)', host.invoke);

    try {
        await assert.rejects(() => commit([{ mes: 'conflict' }]), error => {
            assert.ok(error instanceof Error);
            assert.equal(error.code, 'integrity');
            assert.equal(error.cause, finishError);
            return true;
        });
        assert.equal(host.calls.filter(call => call.command === 'abort_chat_commit').length, 1);
    } finally {
        restore();
    }
});

test('chat payload commit surfaces abort failure with the original error', async () => {
    const host = createCommitHost({
        onAppend: () => { throw new Error('append failed'); },
        abortError: new Error('abort failed'),
    });
    const restore = installRuntime('Mozilla/5.0 (Macintosh)', host.invoke);

    try {
        await assert.rejects(
            () => commit([{ mes: 'failure' }]),
            (error) => error instanceof AggregateError
                && error.errors.some((cause) => cause.message === 'append failed')
                && error.errors.some((cause) => cause.message === 'abort failed'),
        );
    } finally {
        restore();
    }
});

test('chat payload commit does not abort after finish already published', async () => {
    const host = createCommitHost({ finishSizeDelta: 1 });
    const restore = installRuntime('Mozilla/5.0 (Macintosh)', host.invoke);

    try {
        await assert.rejects(() => commit([{ mes: 'finished' }]), /unexpected size/i);
        assert.equal(host.calls.filter((call) => call.command === 'abort_chat_commit').length, 0);
    } finally {
        restore();
    }
});

test('chat payload commit does not classify failed conflict cleanup as an overwriteable error', async () => {
    const finishError = { BadRequest: 'integrity' };
    const abortError = new Error('abort failed');
    const host = createCommitHost({ finishError, abortError });
    const restore = installRuntime('Mozilla/5.0 (Macintosh)', host.invoke);

    try {
        await assert.rejects(() => commit([{ mes: 'conflict' }]), error => {
            assert.ok(error instanceof AggregateError);
            assert.equal(error.code, undefined);
            assert.equal(error.errors[0].cause, finishError);
            assert.equal(error.errors[1], abortError);
            return true;
        });
    } finally {
        restore();
    }
});

for (const route of [
    {
        path: '/api/chats/save',
        body: { ch_name: 'Alice', avatar_url: 'Alice.png', file_name: 'Story' },
        target: TARGET,
        failure: 'Failed to save chat',
    },
    {
        path: '/api/chats/group/save',
        body: { id: 'Story' },
        target: { kind: 'group', chatId: 'Story' },
        failure: 'Failed to save group chat',
    },
]) {
    test(`${route.path} preserves save responses and distinguishes integrity from other failures`, async () => {
        const router = createRouteRegistry();
        registerChatRoutes(router, {
            stripJsonl,
            resolveCharacterId: async () => 'Alice',
        }, { jsonResponse });

        for (const scenario of [
            { status: 200, error: undefined },
            { status: 400, error: 'integrity', finishError: { BadRequest: 'integrity' } },
            { status: 500, error: route.failure, finishError: { InternalServerError: 'Could not read integrity metadata' } },
            { status: 500, error: route.failure, finishError: { BadRequest: 'integrity' }, abortError: new Error('abort failed') },
        ]) {
            const host = createCommitHost({ ...scenario, expectedTarget: route.target });
            const restore = installRuntime('Mozilla/5.0 (Macintosh)', host.invoke);

            try {
                const response = await router.handle({
                    method: 'POST',
                    path: route.path,
                    body: { ...route.body, chat: [{ chat_metadata: { integrity: 'chat' } }, { mes: 'saved' }] },
                });
                assert.equal(response.status, scenario.status);
                const body = await response.json();
                if (scenario.status === 200) {
                    assert.deepEqual(body, { ok: true });
                } else {
                    assert.equal(body.error, scenario.error);
                }
            } finally {
                restore();
            }
        }
    });
}
