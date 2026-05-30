import assert from 'node:assert/strict';
import test from 'node:test';

import { jsonResponse } from '../src/tauri/main/http-utils.js';
import { createRouteRegistry } from '../src/tauri/main/router.js';
import { registerUserRoutes } from '../src/tauri/main/routes/user-routes.js';

function createUserRouter(context) {
    const router = createRouteRegistry();
    registerUserRoutes(router, context, { jsonResponse });
    return router;
}

function bytesStream(bytes) {
    return new ReadableStream({
        start(controller) {
            controller.enqueue(Uint8Array.from(bytes));
            controller.close();
        },
    });
}

test('/api/users/backup streams the staged archive by default', async () => {
    const calls = [];
    const router = createUserRouter({
        safeInvoke: async (command, args) => {
            calls.push({ command, args });
            if (command === 'read_secret_settings') {
                return { allowKeysExposure: false };
            }
            if (command === 'export_user_backup_archive') {
                return {
                    file_name: 'default-user-20260517-120000.zip',
                    archive_path: '/tmp/default-user-20260517-120000.zip',
                };
            }
            if (command === 'cleanup_user_backup_archive') {
                return null;
            }
            throw new Error(`Unexpected command: ${command}`);
        },
        createReadableFileStream: async (filePath) => {
            assert.equal(filePath, '/tmp/default-user-20260517-120000.zip');
            return bytesStream([0x50, 0x4b, 0x03, 0x04]);
        },
    });

    const response = await router.handle({
        method: 'POST',
        path: '/api/users/backup',
        body: { handle: 'default-user' },
    });

    assert.ok(response);
    assert.equal(response.status, 200);
    assert.equal(response.headers.get('Content-Type'), 'application/zip');
    assert.equal(
        response.headers.get('Content-Disposition'),
        'attachment; filename="default-user-20260517-120000.zip"',
    );
    assert.deepEqual(
        Array.from(new Uint8Array(await response.arrayBuffer())),
        [0x50, 0x4b, 0x03, 0x04],
    );
    assert.deepEqual(calls, [
        { command: 'read_secret_settings', args: undefined },
        {
            command: 'export_user_backup_archive',
            args: { handle: 'default-user', include_secrets: false },
        },
        {
            command: 'cleanup_user_backup_archive',
            args: {
                archive_path: '/tmp/default-user-20260517-120000.zip',
            },
        },
    ]);
});

test('/api/users/backup native mode saves the staged archive without a binary body', async () => {
    const calls = [];
    const router = createUserRouter({
        safeInvoke: async (command, args) => {
            calls.push({ command, args });
            if (command === 'read_secret_settings') {
                return { allowKeysExposure: true };
            }
            if (command === 'export_user_backup_archive') {
                return { file_name: 'default-user.zip', archive_path: '/tmp/default-user.zip' };
            }
            return '/downloads/default-user.zip';
        },
    });

    const response = await router.handle({
        method: 'POST',
        path: '/api/users/backup',
        body: { handle: 'default-user', native: true },
    });

    assert.ok(response);
    assert.equal(response.status, 200);
    assert.deepEqual(await response.json(), {
        ok: true,
        mode: 'desktop-native',
        file_name: 'default-user.zip',
        saved_target: '/downloads/default-user.zip',
        includes_secrets: true,
    });
    assert.deepEqual(calls.find((call) => call.command === 'export_user_backup_archive'), {
        command: 'export_user_backup_archive',
        args: { handle: 'default-user', include_secrets: true },
    });
    assert.deepEqual(calls.at(-1), {
        command: 'save_user_backup_archive',
        args: {
            archive_path: '/tmp/default-user.zip',
            file_name: 'default-user.zip',
        },
    });
});

test('/api/users/backup fails instead of returning a pathless archive result', async () => {
    const router = createUserRouter({
        safeInvoke: async (command) => {
            if (command === 'read_secret_settings') {
                return { allowKeysExposure: false };
            }
            return { file_name: 'default-user.zip', archive_path: '' };
        },
    });

    const response = await router.handle({
        method: 'POST',
        path: '/api/users/backup',
        body: { handle: 'default-user' },
    });

    assert.ok(response);
    assert.equal(response.status, 500);
    assert.deepEqual(await response.json(), {
        error: 'Internal server error: User backup archive path is missing',
    });
});

test('/api/users/backup returns JSON errors for invalid requests', async () => {
    const router = createUserRouter({
        safeInvoke: async () => {
            throw new Error('safeInvoke should not be called without a user handle');
        },
    });

    const response = await router.handle({
        method: 'POST',
        path: '/api/users/backup',
        body: {},
    });

    assert.ok(response);
    assert.equal(response.status, 400);
    assert.deepEqual(await response.json(), {
        error: 'Bad request: User handle is required for backup',
    });
});
