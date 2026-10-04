import { installHostIdentity, HOSTS } from './helpers/host-identity.mjs';
import assert from 'node:assert/strict';
import test from 'node:test';

import { createUploadService } from '../src/tauri/main/services/uploads/upload-service.js';

function installRuntimeGlobals(identity) {
    const previousWindow = globalThis.window;
    const restoreIdentity = installHostIdentity(identity);
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
        restoreIdentity();
        if (previousWindow === undefined) {
            delete globalThis.window;
        } else {
            globalThis.window = previousWindow;
        }
    };
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
    const restore = installRuntimeGlobals(HOSTS.android);
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
    const restore = installRuntimeGlobals(HOSTS.ios);
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


test('Desktop upload materialization keeps real file paths without staging copy', async () => {
    const restore = installRuntimeGlobals(HOSTS.macos);
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


test('Android data archive materialization rejects the generic Blob upload path', async () => {
    const restore = installRuntimeGlobals(HOSTS.android);
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
            error: 'Mobile data archive imports must use the native archive picker',
            isTemporary: false,
        });
        assert.deepEqual(calls, []);
    } finally {
        restore();
    }
});

test('Android host staging failure discards the partial staged file', async () => {
    const restore = installRuntimeGlobals(HOSTS.android);
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
