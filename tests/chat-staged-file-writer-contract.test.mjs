import assert from 'node:assert/strict';
import test from 'node:test';

import { writeTempFileFromBytesIterable } from '../src/scripts/tauri/chat/asset-io.js';

const FILE_PATH = '/cache/tauritavern-upload-staging/chat-jsonl/chat.jsonl';

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

function createStagingHost({ chunkSize = 4, onChunk, finishSizeDelta = 0 } = {}) {
    const calls = [];
    const frames = [];

    return {
        calls,
        frames,
        invoke: async (command, args, options) => {
            calls.push({ command, args, options });

            if (command === 'stage_upload_begin') {
                assert.deepEqual(args, {
                    dto: {
                        kind: 'chat-jsonl',
                        preferred_extension: 'jsonl',
                    },
                });
                return { file_path: FILE_PATH, chunk_size: chunkSize };
            }

            if (command === 'stage_upload_chunk') {
                const offset = Number(options?.headers?.offset);
                assert.equal(options?.headers?.['file-path'], encodeURIComponent(FILE_PATH));

                const bytes = options?.headers?.['chunk-encoding'] === 'base64'
                    ? new Uint8Array(Buffer.from(args.data, 'base64'))
                    : args;
                frames.push({ offset, bytes });
                return onChunk
                    ? onChunk({ offset, bytes, index: frames.length - 1 })
                    : offset + bytes.byteLength;
            }

            if (command === 'stage_upload_finish') {
                assert.equal(args.filePath, FILE_PATH);
                return {
                    file_path: FILE_PATH,
                    size: args.expectedSize + finishSizeDelta,
                };
            }

            if (command === 'stage_upload_discard') {
                assert.equal(args.filePath, FILE_PATH);
                return undefined;
            }

            throw new Error(`Unexpected command: ${command}`);
        },
    };
}

test('chat staged writer uses bounded base64 frames on Android', async () => {
    const host = createStagingHost();
    const restore = installRuntime('Mozilla/5.0 (Linux; Android 14)', host.invoke);

    try {
        const input = Uint8Array.from({ length: 10 }, (_, index) => index);
        const tempFile = await writeTempFileFromBytesIterable([input]);
        const chunkCalls = host.calls.filter((call) => call.command === 'stage_upload_chunk');

        assert.deepEqual(host.frames.map((frame) => frame.offset), [0, 4, 8]);
        assert.deepEqual(host.frames.map((frame) => frame.bytes.byteLength), [4, 4, 2]);
        assert.deepEqual(
            Buffer.concat(host.frames.map((frame) => Buffer.from(frame.bytes))),
            Buffer.from(input),
        );
        assert.ok(chunkCalls.every((call) => typeof call.args.data === 'string'));
        assert.ok(chunkCalls.every((call) => call.options.headers['chunk-encoding'] === 'base64'));
        assert.ok(host.calls.every((call) => call.command !== 'plugin:fs|write_file'));

        await tempFile.cleanup();
        assert.equal(host.calls.at(-1).command, 'stage_upload_discard');
    } finally {
        restore();
    }
});

test('chat staged writer keeps one frame in flight', async () => {
    let releaseFirstChunk;
    let markFirstChunkStarted;
    const firstChunkStarted = new Promise((resolve) => {
        markFirstChunkStarted = resolve;
    });
    const host = createStagingHost({
        onChunk: ({ offset, bytes, index }) => {
            if (index !== 0) {
                return offset + bytes.byteLength;
            }

            return new Promise((resolve) => {
                releaseFirstChunk = () => resolve(offset + bytes.byteLength);
                markFirstChunkStarted();
            });
        },
    });
    const restore = installRuntime('Mozilla/5.0 (Linux; Android 14)', host.invoke);

    try {
        const pending = writeTempFileFromBytesIterable([new Uint8Array(10)]);
        await firstChunkStarted;
        assert.equal(host.frames.length, 1);

        releaseFirstChunk();
        const tempFile = await pending;
        assert.equal(host.frames.length, 3);
        await tempFile.cleanup();
    } finally {
        restore();
    }
});

test('chat staged writer discards partial files on protocol failures', async () => {
    const scenarios = [
        {
            name: 'offset mismatch',
            host: () => createStagingHost({
                onChunk: ({ offset, bytes }) => offset + bytes.byteLength + 1,
            }),
            error: /unexpected offset/i,
        },
        {
            name: 'chunk rejection',
            host: () => createStagingHost({
                onChunk: () => {
                    throw new Error('simulated chunk failure');
                },
            }),
            error: /simulated chunk failure/i,
        },
        {
            name: 'finish mismatch',
            host: () => createStagingHost({ finishSizeDelta: 1 }),
            error: /unexpected size/i,
        },
    ];

    for (const scenario of scenarios) {
        const host = scenario.host();
        const restore = installRuntime('Mozilla/5.0 (Linux; Android 14)', host.invoke);

        try {
            await assert.rejects(
                () => writeTempFileFromBytesIterable([new Uint8Array(6)]),
                scenario.error,
                scenario.name,
            );
            assert.equal(
                host.calls.filter((call) => call.command === 'stage_upload_discard').length,
                1,
            );
        } finally {
            restore();
        }
    }
});

test('chat staged writer keeps raw root frames off Android', async () => {
    const host = createStagingHost();
    const restore = installRuntime('Mozilla/5.0 (Macintosh; Intel Mac OS X 14_0)', host.invoke);

    try {
        const tempFile = await writeTempFileFromBytesIterable([new Uint8Array(10)]);
        const chunkCalls = host.calls.filter((call) => call.command === 'stage_upload_chunk');

        assert.deepEqual(host.frames.map((frame) => frame.bytes.byteLength), [4, 4, 2]);
        assert.ok(chunkCalls.every((call) => call.args instanceof Uint8Array));
        assert.ok(chunkCalls.every((call) => call.options.headers['chunk-encoding'] === undefined));

        await tempFile.cleanup();
    } finally {
        restore();
    }
});
