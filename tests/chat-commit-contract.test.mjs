import { installHostIdentity, HOSTS } from './helpers/host-identity.mjs';
import assert from 'node:assert/strict';
import test from 'node:test';

import { commitChatMetadata, commitChatPayload } from '../src/scripts/tauri/chat/commit.js';
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

function installRuntime(identity, invoke) {
    const previousWindow = globalThis.window;
    const restoreIdentity = installHostIdentity(identity);
    globalThis.window = { __TAURI__: { core: { invoke } } };
    return () => {
        restoreIdentity();
        if (previousWindow === undefined) {
            delete globalThis.window;
        } else {
            globalThis.window = previousWindow;
        }
    };
}

function createCommitHost({
    maxFrameBytes = 4,
    onAppend,
    expectedColdSourceId,
    finishError,
    abortError,
    expectedTarget = TARGET,
} = {}) {
    const calls = [];
    const frames = [];

    return {
        calls,
        frames,
        invoke: async (command, args, options) => {
            calls.push({ command, args, options });

            if (command === 'begin_chat_commit') {
                assert.deepEqual(args.target, expectedTarget);
                if (expectedColdSourceId !== undefined) assert.equal(args.operation.coldSourceId, expectedColdSourceId);
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
                if (finishError) {
                    throw finishError;
                }
                return undefined;
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

test('metadata saves capture nested values before asynchronous transport', async () => {
    const release = Promise.withResolvers();
    const host = createCommitHost();
    const restore = installRuntime(HOSTS.windows, async (...args) => {
        await release.promise;
        return host.invoke(...args);
    });
    const chatMetadata = { variables: { score: 1 } };
    try {
        const pending = commitChatMetadata({ target: TARGET, chatMetadata });
        chatMetadata.variables.score = 2;
        release.resolve();
        await pending;
        assert.deepEqual(JSON.parse(Buffer.concat(host.frames.map(frame => frame.bytes)).toString()), {
            variables: { score: 1 },
        });
    } finally {
        restore();
    }
});

test('chat transport preserves UTF-8 across bounded native and Android frames', async () => {
    const payload = [{ chat_metadata: {} }, { mes: '你好 👋\nnext line'.repeat(20) }, {}];
    for (const identity of [HOSTS.windows, HOSTS.android]) {
        const host = createCommitHost({ maxFrameBytes: 7 });
        const restore = installRuntime(identity, host.invoke);
        try {
            await commit(payload);
            assert.ok(host.frames.every(frame => frame.bytes.byteLength <= 7));
            let offset = 0;
            for (const frame of host.frames) {
                assert.equal(frame.offset, offset);
                offset += frame.bytes.byteLength;
            }
            const appends = host.calls.filter(call => call.command === 'append_chat_commit_chunk');
            assert.ok(appends.every(call => identity.platform === 'android'
                ? typeof call.args.data === 'string'
                : call.args instanceof Uint8Array));
            assert.equal(Buffer.concat(host.frames.map(frame => frame.bytes)).toString(), payload.map(JSON.stringify).join('\n'));
        } finally {
            restore();
        }
    }
});

test('chat save snapshots nested values and keeps one frame in flight', async () => {
    let releaseFirst;
    let markStarted;
    const started = new Promise((resolve) => { markStarted = resolve; });
    const host = createCommitHost({
        onAppend: ({ offset, bytes, index }) => index === 0
            ? new Promise((resolve) => {
                releaseFirst = () => resolve(offset + bytes.byteLength);
                markStarted();
            })
            : offset + bytes.byteLength,
    });
    const restore = installRuntime(HOSTS.android, host.invoke);
    const payload = [
        { chat_metadata: { integrity: '10000000-0000-4000-8000-000000000002', variables: { score: 1 } } },
        { mes: 'first message', extra: { extension: { enabled: true } } },
        { mes: 'last message', swipes: ['original swipe'] },
    ];
    const expected = payload.map(entry => JSON.stringify(entry)).join('\n');

    try {
        const pending = commit(payload);
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

test('chat payload commit aborts ACK failures', async () => {
    const host = createCommitHost({
        onAppend: ({ offset, bytes }) => offset + bytes.byteLength + 1,
    });
    const restore = installRuntime(HOSTS.macos, host.invoke);

    try {
        await assert.rejects(() => commit([{ mes: '012345' }]), /unexpected offset/i);
        assert.equal(host.calls.filter((call) => call.command === 'abort_chat_commit').length, 1);
    } finally {
        restore();
    }
});

test('chat payload commit surfaces abort failure with the original error', async () => {
    const host = createCommitHost({
        onAppend: () => { throw new Error('append failed'); },
        abortError: new Error('abort failed'),
    });
    const restore = installRuntime(HOSTS.macos, host.invoke);

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
            { status: 200, body: { ok: true } },
            {
                status: 400,
                body: { error: 'integrity' },
                finishError: { BadRequest: 'integrity' },
            },
            {
                status: 500,
                body: { error: route.failure, details: '{"InternalServerError":"Could not read integrity metadata"}' },
                finishError: { InternalServerError: 'Could not read integrity metadata' },
            },
        ]) {
            const host = createCommitHost({ finishError: scenario.finishError, expectedTarget: route.target });
            const restore = installRuntime(HOSTS.macos, host.invoke);

            try {
                const response = await router.handle({
                    method: 'POST',
                    path: route.path,
                    body: { ...route.body, chat: [{ chat_metadata: { integrity: '10000000-0000-4000-8000-000000000002' } }, { mes: 'saved' }] },
                });
                assert.equal(response.status, scenario.status);
                assert.deepEqual(await response.json(), scenario.body);
            } finally {
                restore();
            }
        }
    });
}

test('cold commit captures source references before asynchronous transport', async () => {
    const payload = [{ chat_metadata:{}, tt_swipe_cold:{opaque_header_field:true} }, {mes:'active',swipe_id:1,swipes:[null,'active'],swipe_info:[null,{}],tt_swipe_cold:{sourceId:7,record:3}}];
    const host = createCommitHost({ expectedColdSourceId:7 });
    const restore = installRuntime(HOSTS.windows, host.invoke);
    try {
        const pending = commit(payload);
        payload[1].tt_swipe_cold.sourceId = 99;
        payload[1].mes = 'later';
        await pending;
        const sent = Buffer.concat(host.frames.map(frame => Buffer.from(frame.bytes))).toString();
        assert.deepEqual(JSON.parse(sent.split('\n')[0]), payload[0]);
        const message = JSON.parse(sent.split('\n')[1]);
        assert.equal(message.tt_swipe_cold.sourceId, 7);
        assert.equal(message.mes, 'active');
    } finally { restore(); }
});
