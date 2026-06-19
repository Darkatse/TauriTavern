import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import test from 'node:test';

import { jsonResponse, textResponse } from '../src/tauri/main/http-utils.js';
import { createRouteRegistry } from '../src/tauri/main/router.js';
import { registerResourceRoutes } from '../src/tauri/main/routes/resource-routes.js';

const backgroundsSource = await readFile(new URL('../src/scripts/backgrounds.js', import.meta.url), 'utf8');

function createResourceRouter(context) {
    const router = createRouteRegistry();
    registerResourceRoutes(router, context, { jsonResponse, textResponse });
    return router;
}

test('/api/backgrounds/folders preserves upstream folder payload shape', async () => {
    const router = createResourceRouter({
        safeInvoke: async (command) => {
            assert.equal(command, 'get_background_folders');
            return {
                folders: [{ id: 'folder-1', name: 'Scenes', thumbnailFile: 'a.png' }],
                imageFolderMap: { 'a.png': ['folder-1'] },
            };
        },
    });

    const response = await router.handle({
        method: 'POST',
        path: '/api/backgrounds/folders',
        url: new URL('http://localhost/api/backgrounds/folders'),
        body: {},
    });

    assert.ok(response);
    assert.equal(response.status, 200);
    assert.deepEqual(await response.json(), {
        folders: [{ id: 'folder-1', name: 'Scenes', thumbnailFile: 'a.png' }],
        imageFolderMap: { 'a.png': ['folder-1'] },
    });
});

test('/api/image-metadata/folders/update maps thumbnailFile to Rust dto field', async () => {
    const calls = [];
    const router = createResourceRouter({
        safeInvoke: async (command, args) => {
            calls.push({ command, args });
            return { id: args.dto.id, name: args.dto.name, thumbnailFile: args.dto.thumbnail_file };
        },
    });

    const response = await router.handle({
        method: 'POST',
        path: '/api/image-metadata/folders/update',
        url: new URL('http://localhost/api/image-metadata/folders/update'),
        body: { id: 'folder-1', name: 'Scenes', thumbnailFile: 'a.png' },
    });

    assert.ok(response);
    assert.equal(response.status, 200);
    assert.deepEqual(calls, [{
        command: 'update_image_metadata_folder',
        args: { dto: { id: 'folder-1', name: 'Scenes', thumbnail_file: 'a.png' } },
    }]);
    assert.deepEqual(await response.json(), { id: 'folder-1', name: 'Scenes', thumbnailFile: 'a.png' });
});

test('/api/image-metadata/folders/set-thumbnails maps batch updates', async () => {
    const calls = [];
    const router = createResourceRouter({
        safeInvoke: async (command, args) => {
            calls.push({ command, args });
        },
    });

    const response = await router.handle({
        method: 'POST',
        path: '/api/image-metadata/folders/set-thumbnails',
        url: new URL('http://localhost/api/image-metadata/folders/set-thumbnails'),
        body: { updates: [{ id: 'folder-1', thumbnailFile: 'a.png' }] },
    });

    assert.ok(response);
    assert.equal(response.status, 200);
    assert.deepEqual(await response.json(), { ok: true });
    assert.deepEqual(calls, [{
        command: 'set_image_metadata_folder_thumbnails',
        args: { dto: { updates: [{ id: 'folder-1', thumbnail_file: 'a.png' }] } },
    }]);
});

test('/api/image-metadata/folders/assign rejects missing paths array', async () => {
    const router = createResourceRouter({
        safeInvoke: async () => {
            throw new Error('safeInvoke should not be called');
        },
    });

    const response = await router.handle({
        method: 'POST',
        path: '/api/image-metadata/folders/assign',
        url: new URL('http://localhost/api/image-metadata/folders/assign'),
        body: { id: 'folder-1', paths: 'backgrounds/a.png' },
    });

    assert.ok(response);
    assert.equal(response.status, 400);
    assert.deepEqual(await response.json(), { error: '"paths" array is required.' });
});

test('/api/backgrounds/upload leaves filename sanitization to Rust storage boundary', async () => {
    const calls = [];
    const cleanupCalls = [];
    const router = createResourceRouter({
        async materializeUploadFile(file, options) {
            calls.push({ command: 'materializeUploadFile', fileName: file.name, options });
            return {
                filePath: '/tmp/staged-background',
                cleanup: async () => cleanupCalls.push('cleanup'),
            };
        },
        async safeInvoke(command, args) {
            calls.push({ command, args });
            assert.notEqual(command, 'sanitize_filename');
            assert.equal(command, 'upload_background_from_path');
            assert.deepEqual(args, {
                filename: 'CON ',
                file_path: '/tmp/staged-background',
            });
            return 'CON';
        },
        invalidateInvokeAll(scope) {
            calls.push({ command: 'invalidateInvokeAll', scope });
        },
    });

    const body = new FormData();
    body.append('avatar', new File(['image'], 'CON ', { type: 'image/png' }));

    const response = await router.handle({
        method: 'POST',
        path: '/api/backgrounds/upload',
        url: new URL('http://localhost/api/backgrounds/upload'),
        body,
    });

    assert.ok(response);
    assert.equal(response.status, 200);
    assert.equal(await response.text(), 'CON');
    assert.deepEqual(cleanupCalls, ['cleanup']);
    assert.deepEqual(calls, [
        {
            command: 'materializeUploadFile',
            fileName: 'CON ',
            options: { kind: 'background', preferredName: 'CON ' },
        },
        {
            command: 'upload_background_from_path',
            args: { filename: 'CON ', file_path: '/tmp/staged-background' },
        },
        { command: 'invalidateInvokeAll', scope: 'read_thumbnail_asset' },
    ]);
});

test('background folder tile cover failures are reported at tile scope', () => {
    assert.match(backgroundsSource, /getFolderCoverUrl\(folder\)\.then\(coverUrl =>/);
    assert.match(backgroundsSource, /\.catch\(error => \{\s*console\.warn\(`Failed to load background folder cover/);
});

test('background folder payload is validated before mutating UI state', () => {
    assert.match(backgroundsSource, /function assertBackgroundFoldersPayload\(data\)/);
    assert.match(backgroundsSource, /folders must be an array/);
    assert.match(backgroundsSource, /imageFolderMap values must be string arrays/);
    assert.match(backgroundsSource, /assertBackgroundFoldersPayload\(data\);\s*folderList = data\.folders;/);
});
