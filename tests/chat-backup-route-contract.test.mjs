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

test('/api/backups/chat/get uses the metadata-only catalog only when requested', async () => {
    const calls = [];
    const router = createBackupsRouter({
        safeInvoke: async (command) => {
            calls.push(command);
            if (command === 'list_chat_backup_catalog') {
                return [
                    {
                        file_name: 'chat_alice_20260722-120000.jsonl',
                        stored_size: 1536,
                        backup_date: 1234,
                        message_count: 7,
                    },
                    {
                        file_name: 'chat_legacy_20260721-120000.jsonl',
                        stored_size: 512,
                        backup_date: 1000,
                    },
                ];
            }
            return [{
                file_name: 'chat_legacy_20260722-120000.jsonl',
                file_size: 2048,
                message_count: 3,
                preview: 'legacy preview',
                date: 4321,
            }];
        },
        ensureJsonl: (name) => name,
        formatFileSize: (size) => `${size} bytes`,
    });

    const catalogResponse = await router.handle({
        method: 'POST',
        path: '/api/backups/chat/get',
        body: { detail: 'catalog' },
    });
    assert.deepEqual(await catalogResponse.json(), [
        {
            file_name: 'chat_alice_20260722-120000.jsonl',
            file_size: '1536 bytes',
            backup_date: 1234,
            message_count: 7,
        },
        {
            file_name: 'chat_legacy_20260721-120000.jsonl',
            file_size: '512 bytes',
            backup_date: 1000,
        },
    ]);

    const legacyResponse = await router.handle({
        method: 'POST',
        path: '/api/backups/chat/get',
        body: {},
    });
    assert.deepEqual(await legacyResponse.json(), [{
        file_name: 'chat_legacy_20260722-120000.jsonl',
        file_size: '2048 bytes',
        chat_items: 3,
        message_count: 3,
        preview_message: 'legacy preview',
        last_mes: 4321,
    }]);
    assert.deepEqual(calls, ['list_chat_backup_catalog', 'list_chat_backups']);
});

test('/api/backups/chat/download streams the decoded backup resource', async () => {
    const calls = [];
    const router = createBackupsRouter({
        createChatBackupDownloadStream: async (name) => {
            calls.push(name);
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
    assert.deepEqual(calls, ['chat_alice_20260722-120000.jsonl']);
});

test('/api/backups/chat/download maps resource open failures before sending the response', async () => {
    const router = createBackupsRouter({
        createChatBackupDownloadStream: async () => {
            throw new Error('Chat backup not found');
        },
    });

    const response = await router.handle({
        method: 'POST',
        path: '/api/backups/chat/download',
        body: { name: 'missing.jsonl' },
    });

    assert.equal(response.status, 404);
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
    assert.match(source, /JSON\.stringify\(\{ detail: 'catalog' \}\)/);
    assert.doesNotMatch(source, /response\.blob\(\)|new File\(\[blob\]/);
    assert.match(routeSource, /createChatBackupDownloadStream\(name\)/);
    assert.doesNotMatch(routeSource, /createReadableFileStream|chat_backup_materialization/);
    assert.match(commandSource, /open_chat_backup_download/);
    assert.match(commandSource, /read_chat_backup_download/);
    assert.doesNotMatch(commandSource, /materialize_chat_backup|discard_chat_backup_materialization/);
});
