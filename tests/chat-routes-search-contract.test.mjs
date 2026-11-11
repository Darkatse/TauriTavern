import assert from 'node:assert/strict';
import test from 'node:test';
import { ensureJsonl as kernelEnsureJsonl, stripJsonl as kernelStripJsonl } from '../src/tauri/main/kernel/chat-utils.js';

import { jsonResponse } from '../src/tauri/main/http-utils.js';
import { createRouteRegistry } from '../src/tauri/main/router.js';
import { registerChatRoutes } from '../src/tauri/main/routes/chat-routes.js';

function createSearchRouteHarness({ group = null } = {}) {
    const router = createRouteRegistry();
    const context = {
        ensureJsonl: kernelEnsureJsonl,
        stripJsonl: kernelStripJsonl,
        formatFileSize: (value) => `${value} bytes`,
        resolveCharacterId: async () => 'alice',
        safeInvoke: async (command, args) => {
            if (command === 'get_group') {
                return group;
            }
            if (command === 'search_chats') return [];
            const names = command === 'list_group_chat_summaries' ? args.chat_ids : ['session.jsonl'];
            return names.map(file_name => ({
                file_name,
                file_size: 1024,
                message_count: 7,
                preview: 'latest',
                date: 1770000000000,
            }));
        },
    };

    registerChatRoutes(router, context, { jsonResponse });

    return { router };
}


test('/api/chats/search returns the legacy list shape for an empty query', async () => {
    const { router } = createSearchRouteHarness();

    const response = await router.handle({
        method: 'POST',
        path: '/api/chats/search',
        body: { query: '   ', avatar_url: 'alice.png' },
    });

    assert.ok(response);
    assert.equal(response.status, 200);
    assert.deepEqual(await response.json(), [{
        file_name: 'session',
        file_size: '1024 bytes',
        message_count: 7,
        preview_message: 'latest',
        last_mes: 1770000000000,
    }]);
});
test('/api/chats/search returns only matches for a non-empty query', async () => {
    const { router } = createSearchRouteHarness();

    const response = await router.handle({
        method: 'POST',
        path: '/api/chats/search',
        body: { query: 'dragon', avatar_url: 'alice.png' },
    });

    assert.deepEqual(await response.json(), []);
});

test('/api/chats/search preserves upstream-significant group chat id spaces', async () => {
    const { router } = createSearchRouteHarness({
        group: { id: 'party', chats: [' group-a ', 'group-b'] },
    });

    const response = await router.handle({
        method: 'POST',
        path: '/api/chats/search',
        body: { query: '', group_id: 'party' },
    });

    assert.deepEqual((await response.json()).map(chat => chat.file_name), [' group-a ', 'group-b']);
});

test('/api/chats/rename returns the backend-committed character chat stem', async () => {
    const router = createRouteRegistry();
    const context = {
        stripJsonl: kernelStripJsonl,
        resolveCharacterId: async () => 'alice',
        safeInvoke: async () => 'CleanName',
    };

    registerChatRoutes(router, context, { jsonResponse });

    const response = await router.handle({
        method: 'POST',
        path: '/api/chats/rename',
        body: {
            avatar_url: 'alice.png',
            original_file: 'Old Name.jsonl',
            renamed_file: 'Clean/Name.jsonl',
        },
    });

    assert.equal(response.status, 200);
    assert.deepEqual(await response.json(), { ok: true, sanitizedFileName: 'CleanName' });
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
