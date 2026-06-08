import assert from 'node:assert/strict';
import test from 'node:test';

import { createUploadService } from '../src/tauri/main/services/uploads/upload-service.js';

function installRuntimeGlobals({ userAgent, platform = '', maxTouchPoints = 0 }) {
    const previousWindow = globalThis.window;
    const previousNavigator = globalThis.navigator;

    Object.defineProperty(globalThis, 'navigator', {
        value: { userAgent, platform, maxTouchPoints },
        configurable: true,
    });
    globalThis.window = {
        __TAURI__: {
            core: {
                invoke: async () => {
                    throw new Error('raw invoke should not be used');
                },
            },
        },
    };

    return () => {
        if (previousNavigator === undefined) {
            delete globalThis.navigator;
        } else {
            Object.defineProperty(globalThis, 'navigator', {
                value: previousNavigator,
                configurable: true,
            });
        }

        if (previousWindow === undefined) {
            delete globalThis.window;
        } else {
            globalThis.window = previousWindow;
        }
    };
}

function installAndroidRuntimeGlobals() {
    return installRuntimeGlobals({
        userAgent: 'Mozilla/5.0 (Linux; Android 14)',
    });
}

function installIosRuntimeGlobals() {
    return installRuntimeGlobals({
        userAgent: 'Mozilla/5.0 (iPhone; CPU iPhone OS 17_0 like Mac OS X)',
        platform: 'iPhone',
        maxTouchPoints: 5,
    });
}

function installDesktopRuntimeGlobals() {
    return installRuntimeGlobals({
        userAgent: 'Mozilla/5.0 (Macintosh; Intel Mac OS X 14_0)',
        platform: 'MacIntel',
    });
}

function createBlobWithPath(parts, type, path) {
    const blob = new Blob(parts, { type });
    Object.defineProperty(blob, 'path', {
        value: path,
        configurable: true,
    });
    return blob;
}

function createHostStagingService({
    calls,
    filePath,
    chunkSize = 4,
    expectedBeginDto,
    expectedChunkEncoding = 'raw',
    failChunkOffset = null,
}) {
    return createUploadService({
        safeInvoke: async (command, args) => {
            calls.push({ channel: 'safe', command, args });

            if (command === 'stage_upload_begin') {
                assert.deepEqual(args, { dto: expectedBeginDto });
                return {
                    file_path: filePath,
                    chunk_size: chunkSize,
                };
            }

            if (command === 'stage_upload_finish') {
                assert.deepEqual(args, {
                    file_path: filePath,
                    expected_size: expectedBeginDto.size,
                });
                return {
                    file_path: filePath,
                    size: expectedBeginDto.size,
                };
            }

            if (command === 'stage_upload_discard') {
                return undefined;
            }

            throw new Error(`unexpected safe command: ${command}`);
        },
        invoke: async (command, args, options) => {
            calls.push({ channel: 'raw', command, args, options });

            if (command !== 'stage_upload_chunk') {
                throw new Error(`unexpected raw command: ${command}`);
            }

            assert.equal(options?.headers?.['file-path'], encodeURIComponent(filePath));
            assert.match(String(options?.headers?.offset || ''), /^\d+$/);

            const offset = Number(options.headers.offset);
            if (offset === failChunkOffset) {
                throw new Error('simulated chunk failure');
            }

            if (expectedChunkEncoding === 'base64') {
                assert.equal(options.headers['chunk-encoding'], 'base64');
                assert.equal(typeof args?.data, 'string');
                return offset + Buffer.from(args.data, 'base64').byteLength;
            }

            assert.equal(options.headers['chunk-encoding'], undefined);
            assert.ok(args instanceof Uint8Array);
            return offset + args.byteLength;
        },
    });
}

test('Android upload materialization uses host staging chunks instead of raw fs writes', async () => {
    const restore = installAndroidRuntimeGlobals();
    const calls = [];
    const service = createHostStagingService({
        calls,
        filePath: '/cache/tauritavern-upload-staging/avatar/upload.png',
        expectedChunkEncoding: 'base64',
        expectedBeginDto: {
            kind: 'avatar',
            preferred_extension: 'png',
            size: 10,
        },
    });

    try {
        const fileInfo = await service.materializeUploadFile(
            new Blob(['abcdefghij'], { type: 'image/png' }),
            { kind: 'avatar', preferredName: 'portrait.png' },
        );

        assert.equal(fileInfo.filePath, '/cache/tauritavern-upload-staging/avatar/upload.png');
        assert.equal(fileInfo.isTemporary, true);

        const chunkCalls = calls.filter(call => call.command === 'stage_upload_chunk');
        assert.deepEqual(chunkCalls.map(call => Number(call.options.headers.offset)), [0, 4, 8]);
        assert.deepEqual(chunkCalls.map(call => call.options.headers['chunk-encoding']), ['base64', 'base64', 'base64']);
        assert.deepEqual(chunkCalls.map(call => Buffer.from(call.args.data, 'base64').byteLength), [4, 4, 2]);

        await fileInfo.cleanup();
        assert.equal(calls.at(-1).command, 'stage_upload_discard');
    } finally {
        restore();
    }
});

test('iOS upload materialization uses host staging even when a scoped path is present', async () => {
    const restore = installIosRuntimeGlobals();
    const calls = [];
    const service = createHostStagingService({
        calls,
        filePath: '/cache/tauritavern-upload-staging/avatar/ios-upload.png',
        expectedBeginDto: {
            kind: 'avatar',
            preferred_extension: 'png',
            size: 10,
        },
    });

    try {
        const fileInfo = await service.materializeUploadFile(
            createBlobWithPath(
                ['abcdefghij'],
                'image/png',
                '/private/var/mobile/Library/Mobile Documents/avatar.png',
            ),
            { kind: 'avatar', preferredName: 'portrait.png' },
        );

        assert.equal(fileInfo.filePath, '/cache/tauritavern-upload-staging/avatar/ios-upload.png');
        assert.equal(fileInfo.isTemporary, true);
        assert.deepEqual(calls.map(call => `${call.channel}:${call.command}`), [
            'safe:stage_upload_begin',
            'raw:stage_upload_chunk',
            'raw:stage_upload_chunk',
            'raw:stage_upload_chunk',
            'safe:stage_upload_finish',
        ]);

        await fileInfo.cleanup();
        assert.equal(calls.at(-1).command, 'stage_upload_discard');
    } finally {
        restore();
    }
});

test('iOS data archive materialization rejects the generic Blob upload path', async () => {
    const restore = installIosRuntimeGlobals();
    const calls = [];
    const service = createUploadService({
        safeInvoke: async (command, args) => {
            calls.push({ command, args });
            throw new Error('host staging should not be called for iOS data archives');
        },
    });

    try {
        const fileInfo = await service.materializeUploadFile(
            new Blob(['zip'], { type: 'application/zip' }),
            { kind: 'data-archive', preferredName: 'backup.zip' },
        );

        assert.deepEqual(fileInfo, {
            filePath: '',
            error: 'iOS data archive imports must use the native archive picker',
            isTemporary: false,
        });
        assert.deepEqual(calls, []);
    } finally {
        restore();
    }
});

test('Desktop upload materialization keeps real file paths without staging copy', async () => {
    const restore = installDesktopRuntimeGlobals();
    const calls = [];
    const service = createUploadService({
        safeInvoke: async (command, args) => {
            calls.push({ command, args });
            throw new Error('host staging should not be called for real desktop paths');
        },
    });

    try {
        const fileInfo = await service.materializeUploadFile(
            createBlobWithPath(['avatar'], 'image/png', '/Users/test/Pictures/avatar.png'),
            { kind: 'avatar', preferredName: 'portrait.png' },
        );

        assert.deepEqual(fileInfo, {
            filePath: '/Users/test/Pictures/avatar.png',
            isTemporary: false,
        });
        assert.deepEqual(calls, []);
    } finally {
        restore();
    }
});

test('Desktop Blob upload fallback uses host staging instead of raw fs temp writes', async () => {
    const restore = installDesktopRuntimeGlobals();
    const calls = [];
    const service = createHostStagingService({
        calls,
        filePath: '/tmp/tauritavern-upload-staging/chat-import/upload.jsonl',
        expectedBeginDto: {
            kind: 'chat-import',
            preferred_extension: 'jsonl',
            size: 8,
        },
    });

    try {
        const fileInfo = await service.materializeUploadFile(
            new Blob(['abcdefgh'], { type: 'application/octet-stream' }),
            { kind: 'chat-import', preferredName: 'chat.jsonl' },
        );

        assert.equal(fileInfo.filePath, '/tmp/tauritavern-upload-staging/chat-import/upload.jsonl');
        assert.equal(fileInfo.isTemporary, true);
        assert.equal(typeof fileInfo.cleanup, 'function');
        assert.deepEqual(calls.map(call => `${call.channel}:${call.command}`), [
            'safe:stage_upload_begin',
            'raw:stage_upload_chunk',
            'raw:stage_upload_chunk',
            'safe:stage_upload_finish',
        ]);
    } finally {
        restore();
    }
});

test('Android data archive materialization rejects the generic Blob upload path', async () => {
    const restore = installAndroidRuntimeGlobals();
    const calls = [];
    const service = createUploadService({
        safeInvoke: async (command, args) => {
            calls.push({ command, args });
            throw new Error('host staging should not be called for Android data archives');
        },
    });

    try {
        const fileInfo = await service.materializeUploadFile(
            new Blob(['zip'], { type: 'application/zip' }),
            { kind: 'data-archive', preferredName: 'backup.zip' },
        );

        assert.deepEqual(fileInfo, {
            filePath: '',
            error: 'Android data archive imports must use the native archive picker',
            isTemporary: false,
        });
        assert.deepEqual(calls, []);
    } finally {
        restore();
    }
});

test('Android host staging failure discards the partial staged file', async () => {
    const restore = installAndroidRuntimeGlobals();
    const originalWarn = console.warn;
    const calls = [];
    const service = createHostStagingService({
        calls,
        filePath: '/cache/tauritavern-upload-staging/avatar/upload.png',
        expectedChunkEncoding: 'base64',
        expectedBeginDto: {
            kind: 'avatar',
            preferred_extension: 'png',
            size: 8,
        },
        failChunkOffset: 4,
    });

    try {
        console.warn = () => {};
        const fileInfo = await service.materializeUploadFile(
            new Blob(['abcdefgh'], { type: 'image/png' }),
            { kind: 'avatar', preferredName: 'portrait.png' },
        );

        assert.equal(fileInfo.filePath, '');
        assert.match(fileInfo.error, /simulated chunk failure/);
        assert.equal(calls.at(-1).command, 'stage_upload_discard');
    } finally {
        console.warn = originalWarn;
        restore();
    }
});
