import assert from 'node:assert/strict';
import test from 'node:test';
import { ensureJsonl as kernelEnsureJsonl, stripJsonl as kernelStripJsonl } from '../src/tauri/main/kernel/chat-utils.js';

import { jsonResponse } from '../src/tauri/main/http-utils.js';
import { createRouteRegistry } from '../src/tauri/main/router.js';
import { registerChatRoutes } from '../src/tauri/main/routes/chat-routes.js';

function createSearchRouteHarness({ group = null } = {}) {
    const router = createRouteRegistry();
    const calls = [];
    const context = {
        ensureJsonl: kernelEnsureJsonl,
        stripJsonl: kernelStripJsonl,
        formatFileSize: (value) => `${value} bytes`,
        resolveCharacterId: async () => 'alice',
        safeInvoke: async (command, args) => {
            calls.push({ command, args });
            if (command === 'get_group') {
                return group;
            }
            return [{
                file_name: 'session.jsonl',
                file_size: 1024,
                message_count: 7,
                preview: 'latest',
                date: 1770000000000,
            }];
        },
    };

    registerChatRoutes(router, context, { jsonResponse });

    return { router, calls };
}

test('kernel chat file helpers preserve uppercase JSONL as upstream stem text', () => {
    assert.equal(kernelEnsureJsonl('Story.JSONL'), 'Story.JSONL.jsonl');
    assert.equal(kernelEnsureJsonl('Story'), 'Story.jsonl');
    assert.equal(kernelStripJsonl(' Story.JSONL'), ' Story.JSONL');
    assert.equal(kernelStripJsonl(' Story.JSONL.jsonl'), ' Story.JSONL');
    assert.equal(kernelStripJsonl('Story.JSONL '), 'Story.JSONL ');
});

test('/api/chats/search uses summary listing for empty character query', async () => {
    const { router, calls } = createSearchRouteHarness();

    const response = await router.handle({
        method: 'POST',
        path: '/api/chats/search',
        body: { query: '   ', avatar_url: 'alice.png' },
    });

    assert.ok(response);
    assert.equal(response.status, 200);
    assert.deepEqual(calls, [{
        command: 'list_chat_summaries',
        args: {
            character_filter: 'alice',
            include_metadata: false,
        },
    }]);
    assert.deepEqual(await response.json(), [{
        file_name: 'session',
        file_size: '1024 bytes',
        message_count: 7,
        preview_message: 'latest',
        last_mes: 1770000000000,
    }]);
});

test('/api/chats/search keeps full search command for non-empty character query', async () => {
    const { router, calls } = createSearchRouteHarness();

    await router.handle({
        method: 'POST',
        path: '/api/chats/search',
        body: { query: 'dragon', avatar_url: 'alice.png' },
    });

    assert.deepEqual(calls, [{
        command: 'search_chats',
        args: {
            query: 'dragon',
            characterFilter: 'alice',
        },
    }]);
});

test('/api/chats/search uses group summary listing for empty group query', async () => {
    const { router, calls } = createSearchRouteHarness({
        group: { id: 'party', chats: ['group-a', 'group-b'] },
    });

    const response = await router.handle({
        method: 'POST',
        path: '/api/chats/search',
        body: { query: '', group_id: 'party' },
    });

    assert.deepEqual(calls, [
        {
            command: 'get_group',
            args: { id: 'party' },
        },
        {
            command: 'list_group_chat_summaries',
            args: {
                chat_ids: ['group-a', 'group-b'],
                include_metadata: false,
            },
        },
    ]);
    assert.deepEqual(await response.json(), [{
        file_name: 'session',
        file_size: '1024 bytes',
        message_count: 7,
        preview_message: 'latest',
        last_mes: 1770000000000,
    }]);
});

test('/api/chats/search preserves upstream-significant group chat id spaces', async () => {
    const { router, calls } = createSearchRouteHarness({
        group: { id: 'party', chats: [' group-a ', 'group-b'] },
    });

    await router.handle({
        method: 'POST',
        path: '/api/chats/search',
        body: { query: '', group_id: 'party' },
    });

    assert.deepEqual(calls.at(-1), {
        command: 'list_group_chat_summaries',
        args: {
            chat_ids: [' group-a ', 'group-b'],
            include_metadata: false,
        },
    });
});

test('/api/chats/rename returns the backend-committed character chat stem', async () => {
    const router = createRouteRegistry();
    const calls = [];
    const context = {
        stripJsonl: kernelStripJsonl,
        resolveCharacterId: async ({ avatar }) => {
            calls.push({ command: 'resolveCharacterId', args: { avatar } });
            return 'alice';
        },
        safeInvoke: async (command, args) => {
            calls.push({ command, args });
            return 'Clean Name';
        },
    };

    registerChatRoutes(router, context, { jsonResponse });

    const response = await router.handle({
        method: 'POST',
        path: '/api/chats/rename',
        body: {
            avatar_url: 'alice.png',
            original_file: 'Old Name.jsonl',
            renamed_file: 'Clean Name.jsonl',
        },
    });

    assert.equal(response.status, 200);
    assert.deepEqual(calls, [
        { command: 'resolveCharacterId', args: { avatar: 'alice.png' } },
        {
            command: 'rename_chat',
            args: {
                dto: {
                    character_name: 'alice',
                    old_file_name: 'Old Name',
                    new_file_name: 'Clean Name',
                },
            },
        },
    ]);
    assert.deepEqual(await response.json(), { ok: true, sanitizedFileName: 'Clean Name' });
});

test('/api/chats/rename preserves upstream-significant file name spaces', async () => {
    const router = createRouteRegistry();
    const calls = [];
    const context = {
        stripJsonl: kernelStripJsonl,
        resolveCharacterId: async () => 'alice',
        safeInvoke: async (command, args) => {
            calls.push({ command, args });
            return ' Story Renamed';
        },
    };

    registerChatRoutes(router, context, { jsonResponse });

    const response = await router.handle({
        method: 'POST',
        path: '/api/chats/rename',
        body: {
            avatar_url: 'alice.png',
            original_file: ' Story.jsonl',
            renamed_file: ' Story Renamed.jsonl',
        },
    });

    assert.equal(response.status, 200);
    assert.deepEqual(calls, [{
        command: 'rename_chat',
        args: {
            dto: {
                character_name: 'alice',
                old_file_name: ' Story',
                new_file_name: ' Story Renamed',
            },
        },
    }]);
});

test('/api/chats/rename returns 400 for invalid avatar_url without backend mutation', async () => {
    const router = createRouteRegistry();
    const context = {
        stripJsonl: kernelStripJsonl,
        resolveCharacterId: async () => {
            throw new Error('Bad request: invalid avatar_url');
        },
        safeInvoke: async () => {
            throw new Error('safeInvoke should not be called');
        },
    };

    registerChatRoutes(router, context, { jsonResponse });

    const response = await router.handle({
        method: 'POST',
        path: '/api/chats/rename',
        body: {
            avatar_url: 'thumbnail?file=alice.png',
            original_file: 'Old Name.jsonl',
            renamed_file: 'Clean Name.jsonl',
        },
    });

    assert.equal(response.status, 400);
    assert.deepEqual(await response.json(), { error: 'invalid avatar_url' });
});

test('/api/chats/rename returns the backend-committed group chat stem', async () => {
    const router = createRouteRegistry();
    const calls = [];
    const context = {
        stripJsonl: kernelStripJsonl,
        safeInvoke: async (command, args) => {
            calls.push({ command, args });
            return 'Group Clean Name';
        },
    };

    registerChatRoutes(router, context, { jsonResponse });

    const response = await router.handle({
        method: 'POST',
        path: '/api/chats/rename',
        body: {
            is_group: true,
            original_file: 'Group Old.jsonl',
            renamed_file: 'Group Clean Name.jsonl',
        },
    });

    assert.equal(response.status, 200);
    assert.deepEqual(calls, [{
        command: 'rename_group_chat',
        args: {
            dto: {
                old_file_name: 'Group Old',
                new_file_name: 'Group Clean Name',
            },
        },
    }]);
    assert.deepEqual(await response.json(), { ok: true, sanitizedFileName: 'Group Clean Name' });
});

test('/api/chats/search keeps group search command for non-empty group query', async () => {
    const { router, calls } = createSearchRouteHarness({
        group: { id: 'party', chats: ['group-a'] },
    });

    await router.handle({
        method: 'POST',
        path: '/api/chats/search',
        body: { query: 'dragon', group_id: 'party' },
    });

    assert.deepEqual(calls, [
        {
            command: 'get_group',
            args: { id: 'party' },
        },
        {
            command: 'search_group_chats',
            args: {
                query: 'dragon',
                chat_ids: ['group-a'],
            },
        },
    ]);
});
