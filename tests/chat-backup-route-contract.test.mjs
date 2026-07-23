import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import test from 'node:test';

import { jsonResponse, textResponse } from '../src/tauri/main/http-utils.js';
import { createRouteRegistry } from '../src/tauri/main/router.js';
import { registerBackupsRoutes } from '../src/tauri/main/routes/backups-routes.js';
import { registerChatRoutes } from '../src/tauri/main/routes/chat-routes.js';

function createBackupsRouter(context) {
    const router = createRouteRegistry();
    registerBackupsRoutes(router, context, { jsonResponse, textResponse });
    return router;
}

test('/api/backups/chat/download streams and discards the decoded materialization at EOF', async () => {
    const calls = [];
    const router = createBackupsRouter({
        safeInvoke: async (command, args) => {
            calls.push({ command, args });
            if (command === 'materialize_chat_backup') {
                return '/tmp/chat-backup-materialized.jsonl';
            }
            if (command === 'discard_chat_backup_materialization') {
                return null;
            }
            throw new Error(`Unexpected command: ${command}`);
        },
        createReadableFileStream: async (path) => {
            assert.equal(path, '/tmp/chat-backup-materialized.jsonl');
            return new ReadableStream({
                start(controller) {
                    controller.enqueue(new TextEncoder().encode('{"mes":"hello"}\n'));
                    controller.close();
                },
            });
        },
    });

    const response = await router.handle({
        method: 'POST',
        path: '/api/backups/chat/download',
        body: { name: 'chat_alice_20260722-120000.jsonl' },
    });

    assert.equal(response.status, 200);
    assert.equal(await response.text(), '{"mes":"hello"}\n');
    assert.deepEqual(calls, [
        {
            command: 'materialize_chat_backup',
            args: { name: 'chat_alice_20260722-120000.jsonl' },
        },
        {
            command: 'discard_chat_backup_materialization',
            args: { path: '/tmp/chat-backup-materialized.jsonl' },
        },
    ]);
});

test('/api/backups/chat/download keeps a completed stream successful when cleanup fails', async () => {
    const calls = [];
    const router = createBackupsRouter({
        safeInvoke: async (command, args) => {
            calls.push({ command, args });
            if (command === 'materialize_chat_backup') {
                return '/tmp/chat-backup-cleanup-error.jsonl';
            }
            if (command === 'discard_chat_backup_materialization') {
                throw new Error('cleanup failed');
            }
            throw new Error(`Unexpected command: ${command}`);
        },
        createReadableFileStream: () => new ReadableStream({
            start(controller) {
                controller.enqueue(new TextEncoder().encode('{"mes":"hello"}\n'));
                controller.close();
            },
        }),
    });

    const response = await router.handle({
        method: 'POST',
        path: '/api/backups/chat/download',
        body: { name: 'chat_alice_20260722-120000.jsonl' },
    });
    const originalWarn = console.warn;
    const warnings = [];
    console.warn = (...args) => warnings.push(args);
    let text;
    try {
        text = await response.text();
    } finally {
        console.warn = originalWarn;
    }

    assert.equal(response.status, 200);
    assert.equal(text, '{"mes":"hello"}\n');
    assert.equal(calls.filter(({ command }) => command === 'discard_chat_backup_materialization').length, 1);
    assert.equal(warnings.length, 1);
});

test('/api/backups/chat/download discards the materialization when the consumer cancels', async () => {
    const calls = [];
    let sourceCanceled = false;
    const router = createBackupsRouter({
        safeInvoke: async (command, args) => {
            calls.push({ command, args });
            if (command === 'materialize_chat_backup') {
                return '/tmp/chat-backup-cancel.jsonl';
            }
            return null;
        },
        createReadableFileStream: () => new ReadableStream({
            start(controller) {
                controller.enqueue(new Uint8Array([1, 2, 3]));
            },
            cancel() {
                sourceCanceled = true;
            },
        }),
    });

    const response = await router.handle({
        method: 'POST',
        path: '/api/backups/chat/download',
        body: { name: 'chat_alice_20260722-120000.jsonl' },
    });
    const reader = response.body.getReader();
    await reader.read();
    await reader.cancel('test cancellation');

    assert.equal(sourceCanceled, true);
    assert.deepEqual(calls.at(-1), {
        command: 'discard_chat_backup_materialization',
        args: { path: '/tmp/chat-backup-cancel.jsonl' },
    });
});

test('/api/backups/chat/download discards the materialization when the source stream fails', async () => {
    const calls = [];
    const router = createBackupsRouter({
        safeInvoke: async (command, args) => {
            calls.push({ command, args });
            if (command === 'materialize_chat_backup') {
                return '/tmp/chat-backup-error.jsonl';
            }
            return null;
        },
        createReadableFileStream: () => new ReadableStream({
            pull(controller) {
                controller.error(new Error('read failed'));
            },
        }),
    });

    const response = await router.handle({
        method: 'POST',
        path: '/api/backups/chat/download',
        body: { name: 'chat_alice_20260722-120000.jsonl' },
    });

    await assert.rejects(() => response.arrayBuffer(), /read failed/);
    assert.deepEqual(calls.at(-1), {
        command: 'discard_chat_backup_materialization',
        args: { path: '/tmp/chat-backup-error.jsonl' },
    });
});

test('/api/chats/import restores a character backup without an upload Blob', async () => {
    const calls = [];
    const router = createRouteRegistry();
    registerChatRoutes(router, {
        resolveCharacterId: async (options) => {
            calls.push({ command: 'resolveCharacterId', args: options });
            return 'alice-id';
        },
        safeInvoke: async (command, args) => {
            calls.push({ command, args });
            return ['Restored Chat.jsonl'];
        },
    }, { jsonResponse });

    const body = new FormData();
    body.set('backup_name', 'chat_alice_20260722-120000.jsonl');
    body.set('avatar_url', 'alice.png');

    const response = await router.handle({ method: 'POST', path: '/api/chats/import', body });

    assert.equal(response.status, 200);
    assert.deepEqual(await response.json(), { res: true, fileNames: ['Restored Chat.jsonl'] });
    assert.deepEqual(calls, [
        {
            command: 'resolveCharacterId',
            args: { avatar: 'alice.png', fallbackName: '' },
        },
        {
            command: 'restore_character_chat_backup',
            args: {
                dto: {
                    backup_name: 'chat_alice_20260722-120000.jsonl',
                    character_name: 'alice-id',
                    character_display_name: 'alice-id',
                },
            },
        },
    ]);
});

test('/api/chats/group/import restores a group backup without an upload Blob', async () => {
    const calls = [];
    const router = createRouteRegistry();
    registerChatRoutes(router, {
        safeInvoke: async (command, args) => {
            calls.push({ command, args });
            return 'Restored Group Chat';
        },
    }, { jsonResponse });

    const body = new FormData();
    body.set('backup_name', 'chat_group_20260722-120000.jsonl');

    const response = await router.handle({ method: 'POST', path: '/api/chats/group/import', body });

    assert.equal(response.status, 200);
    assert.deepEqual(await response.json(), { res: 'Restored Group Chat' });
    assert.deepEqual(calls, [{
        command: 'restore_group_chat_backup',
        args: { dto: { backup_name: 'chat_group_20260722-120000.jsonl' } },
    }]);
});

test('/api/chats/import keeps the upload contract when a Blob also carries backup_name', async () => {
    const calls = [];
    let cleaned = false;
    const router = createRouteRegistry();
    registerChatRoutes(router, {
        resolveCharacterId: async () => 'alice-id',
        materializeUploadFile: async () => ({
            filePath: '/tmp/upload.jsonl',
            cleanup: async () => {
                cleaned = true;
            },
        }),
        safeInvoke: async (command, args) => {
            calls.push({ command, args });
            return ['Uploaded Chat.jsonl'];
        },
    }, { jsonResponse });

    const body = new FormData();
    body.set('backup_name', 'unrelated-extension-field');
    body.set('file_type', 'jsonl');
    body.set('avatar', new Blob(['{"chat_metadata":{}}\n']), 'upload.jsonl');

    const response = await router.handle({ method: 'POST', path: '/api/chats/import', body });

    assert.equal(response.status, 200);
    assert.deepEqual(await response.json(), { res: true, fileNames: ['Uploaded Chat.jsonl'] });
    assert.equal(calls[0].command, 'import_character_chats');
    assert.equal(calls[0].args.dto.file_path, '/tmp/upload.jsonl');
    assert.equal(cleaned, true);
});

test('chat backup browser views through a stream and restores by logical backup name', async () => {
    const source = await readFile(new URL('../src/scripts/chat-backups.js', import.meta.url), 'utf8');
    const routeSource = await readFile(new URL('../src/tauri/main/routes/backups-routes.js', import.meta.url), 'utf8');
    const commandSource = await readFile(new URL('../src/tauri/main/kernel/invokes/tauri-commands.js', import.meta.url), 'utf8');

    assert.match(source, /visitJsonlStream\(response\.body,/);
    assert.match(source, /formData\.set\('backup_name', name\)/);
    assert.doesNotMatch(source, /response\.blob\(\)|new File\(\[blob\]/);
    assert.doesNotMatch(routeSource, /get_chat_backup_raw|normalizeBinaryPayload/);
    assert.doesNotMatch(commandSource, /get_chat_backup_raw/);
});

test('host startup scopes chat staging for portable and custom data roots', async () => {
    const source = await readFile(new URL('../src-tauri/crates/tauritavern/src/app/host/resources.rs', import.meta.url), 'utf8');

    assert.match(source, /\.fs_scope\(\)/);
    assert.match(source, /\.join\("default-user"\)[\s\S]*\.join\("\.staging"\)[\s\S]*\.join\("chat-commits"\)/);
    assert.match(source, /allow_directory\(&chat_staging_root, true\)/);
});
