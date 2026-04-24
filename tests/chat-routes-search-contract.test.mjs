import assert from 'node:assert/strict';
import test from 'node:test';

import { jsonResponse } from '../src/tauri/main/http-utils.js';
import { createRouteRegistry } from '../src/tauri/main/router.js';
import { registerChatRoutes } from '../src/tauri/main/routes/chat-routes.js';

function ensureJsonl(value) {
    const text = String(value || '');
    return /\.jsonl$/i.test(text) ? text : `${text}.jsonl`;
}

function createSearchRouteHarness({ group = null } = {}) {
    const router = createRouteRegistry();
    const calls = [];
    const context = {
        ensureJsonl,
        formatFileSize: (value) => `${value} bytes`,
        resolveCharacterId: async () => 'alice',
        safeInvoke: async (command, args) => {
            calls.push({ command, args });
            if (command === 'get_group') {
                return group;
            }
            return [{
                file_name: 'session',
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
        file_name: 'session.jsonl',
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

    await router.handle({
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
