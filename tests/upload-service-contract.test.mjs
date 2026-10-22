import assert from 'node:assert/strict';
import test from 'node:test';

import { installHostIdentity, HOSTS } from './helpers/host-identity.mjs';
import { createUploadService } from '../src/tauri/main/services/uploads/upload-service.js';

function stagingHost(encoding, failChunkOffset) {
    const filePath = '/cache/staged upload.png';
    const chunks = [];
    const discarded = [];
    let received = 0;
    const service = createUploadService({
        safeInvoke: async (command, args) => {
            if (command === 'stage_file_begin') return { file_path: filePath, chunk_size: 4 };
            if (command === 'stage_file_finish') {
                assert.equal(received, args.expectedSize);
                return { file_path: filePath, size: received };
            }
            if (command === 'stage_file_discard') {
                discarded.push(args.filePath);
                return;
            }
            throw new Error(`Unexpected command: ${command}`);
        },
        invoke: async (command, body, { headers }) => {
            assert.equal(command, 'stage_file_chunk');
            assert.equal(decodeURIComponent(headers['file-path']), filePath);
            assert.equal(headers['chunk-encoding'], encoding);
            assert.equal(Number(headers.offset), received);
            if (received === failChunkOffset) throw new Error('chunk failed');
            const bytes = encoding === 'base64' ? Buffer.from(body.data, 'base64') : body;
            chunks.push(bytes);
            received += bytes.byteLength;
            return received;
        },
    });
    return { service, filePath, chunks, discarded };
}

test('mobile uploads preserve bytes across IPC chunks and own their staged copies', async () => {
    const bytes = Uint8Array.of(0, 255, 128, 64, 1, 2, 3, 4, 5, 6);
    for (const [identity, encoding] of [[HOSTS.android, 'base64'], [HOSTS.ios, undefined]]) {
        const restore = installHostIdentity(identity);
        try {
            const { service, filePath, chunks, discarded } = stagingHost(encoding);
            const blob = new Blob([bytes], { type: 'image/png' });
            // A mobile picker's scoped path cannot substitute for an owned copy.
            Object.defineProperty(blob, 'path', { value: '/private/scoped/avatar.png' });
            const file = await service.materializeUploadFile(blob, { kind: 'avatar', preferredName: 'portrait.png' });

            assert.equal(file.filePath, filePath);
            assert.equal(file.isTemporary, true);
            assert.deepEqual(Buffer.concat(chunks), Buffer.from(bytes));
            assert.deepEqual(discarded, []);
            await file.cleanup();
            assert.deepEqual(discarded, [filePath]);
        } finally {
            restore();
        }
    }
});

test('failed upload discards the partial staged file and reports the failure', async t => {
    t.after(installHostIdentity(HOSTS.android));
    t.mock.method(console, 'warn', () => {});
    const { service, filePath, chunks, discarded } = stagingHost('base64', 4);
    const file = await service.materializeUploadFile(new Blob(['abcdefgh']));

    assert.equal(Buffer.concat(chunks).toString(), 'abcd');
    assert.equal(file.filePath, '');
    assert.match(file.error, /chunk failed/);
    assert.deepEqual(discarded, [filePath]);
});
