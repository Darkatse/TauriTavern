import assert from 'node:assert/strict';
import test from 'node:test';

import { commitChatPayload } from '../src/scripts/tauri/chat/commit.js';
import {
    saveCharacterChatPayload,
    saveGroupChatPayload,
} from '../src/scripts/tauri/chat/transport.js';

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

test('character and group save convergence points send logical targets', async () => {
    const scenarios = [
        {
            host: createCommitHost({
                expectedTarget: {
                    kind: 'character',
                    characterId: 'Alice',
                    fileName: 'Story',
                },
                expectedForce: true,
                expectedCommitReason: 'maintenance',
            }),
            run: () => saveCharacterChatPayload({
                characterName: 'Display Name',
                avatarUrl: 'Alice.png',
                fileName: 'Story.jsonl',
                payload: [{ user_name: 'User' }],
                force: true,
                commitReason: 'maintenance',
            }),
        },
        {
            host: createCommitHost({
                expectedTarget: { kind: 'group', chatId: 'Group Story' },
                expectedCommitReason: 'generationCheckpoint',
            }),
            run: () => saveGroupChatPayload({
                id: 'Group Story.jsonl',
                payload: [{ user_name: 'User' }],
                commitReason: 'generationCheckpoint',
            }),
        },
    ];

    for (const scenario of scenarios) {
        const restore = installRuntime('Mozilla/5.0 (Macintosh)', scenario.host.invoke);
        try {
            await scenario.run();
            assert.ok(scenario.host.calls.every(({ command }) => !command.startsWith('stage_upload_')));
        } finally {
            restore();
        }
    }
});

test('chat payload commit treats a null legacy reason as a mutation', async () => {
    const host = createCommitHost();
    const restore = installRuntime('Mozilla/5.0 (Macintosh)', host.invoke);

    try {
        await commitChatPayload({
            target: TARGET,
            payload: [{ user_name: 'User' }],
            force: false,
            commitReason: null,
        });
    } finally {
        restore();
    }
});

test('chat payload commit keeps one frame in flight', async () => {
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
    const restore = installRuntime('Mozilla/5.0 (Linux; Android 14)', host.invoke);

    try {
        const pending = commit([{ mes: '0123456789' }]);
        await started;
        assert.equal(host.frames.length, 1);

        releaseFirst();
        await pending;
        assert.ok(host.frames.length > 1);
    } finally {
        restore();
    }
});

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

test('chat payload commit aborts generator and ACK failures', async () => {
    const scenarios = [
        {
            payload: [null],
            host: createCommitHost(),
            error: /entry at index 0/i,
        },
        {
            payload: [{ mes: '012345' }],
            host: createCommitHost({
                onAppend: ({ offset, bytes }) => offset + bytes.byteLength + 1,
            }),
            error: /unexpected offset/i,
        },
    ];

    for (const scenario of scenarios) {
        const restore = installRuntime('Mozilla/5.0 (Macintosh)', scenario.host.invoke);
        try {
            await assert.rejects(() => commit(scenario.payload), scenario.error);
            assert.equal(
                scenario.host.calls.filter((call) => call.command === 'abort_chat_commit').length,
                1,
            );
        } finally {
            restore();
        }
    }
});

test('chat payload commit aborts a rejected finish', async () => {
    const host = createCommitHost({ finishError: new Error('finish failed') });
    const restore = installRuntime('Mozilla/5.0 (Macintosh)', host.invoke);

    try {
        await assert.rejects(() => commit([{ mes: 'failure' }]), /finish failed/);
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
