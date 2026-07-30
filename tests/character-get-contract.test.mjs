import assert from 'node:assert/strict';
import test from 'node:test';

import { textResponse, jsonResponse } from '../src/tauri/main/http-utils.js';
import { ensureJsonl, stripJsonl } from '../src/tauri/main/kernel/chat-utils.js';
import { createRouteRegistry } from '../src/tauri/main/router.js';
import { registerCharacterRoutes } from '../src/tauri/main/routes/character-routes.js';
import { createCharacterService } from '../src/tauri/main/services/characters/character-service.js';

function registerGetRoute(context) {
    const router = createRouteRegistry();
    registerCharacterRoutes(router, context, { textResponse, jsonResponse });
    return router;
}

test('/api/characters/all refreshes backend shallow projection instead of reusing mixed JS cache', async () => {
    let listReads = 0;
    const service = createCharacterService({
        safeInvoke: async (command, args) => {
            if (command === 'get_all_characters') {
                listReads += 1;
                assert.deepEqual(args, { shallow: true });
                return [{
                    name: listReads === 1 ? 'Stale Alice' : 'Fresh Alice',
                    avatar: 'Alice.png',
                    shallow: true,
                    data: { extensions: {} },
                }];
            }

            assert.equal(command, 'get_character');
            assert.deepEqual(args, { name: 'Alice' });
            return {
                name: 'Full Alice',
                avatar: 'Alice.png',
                shallow: false,
                description: 'heavy',
                data: { extensions: {} },
            };
        },
    });

    await service.getAllCharacters({ shallow: true, forceRefresh: true });
    await service.getSingleCharacter({ avatar_url: 'Alice.png' });

    const router = registerGetRoute(service);
    const response = await router.handle({
        method: 'POST',
        path: '/api/characters/all',
        url: new URL('http://localhost/api/characters/all'),
        body: {},
    });

    assert.ok(response);
    assert.equal(response.status, 200);
    const body = await response.json();
    assert.equal(body[0].name, 'Fresh Alice');
    assert.equal(body[0].avatar, 'Alice.png');
    assert.equal(body[0].shallow, true);
    assert.equal(body[0].description, '');
    assert.equal(listReads, 2);
});

test('/api/characters/get treats avatar_url as an exact avatar filename identity', async () => {
    const calls = [];
    const service = createCharacterService({
        safeInvoke: async (command, args) => {
            calls.push({ command, args });
            assert.equal(command, 'get_character');
            return {
                name: args.name,
                avatar: `${args.name}.png`,
                data: { extensions: {} },
            };
        },
    });

    const cases = [
        { avatar: 'Alice#1.png', stem: 'Alice#1' },
        { avatar: 'Alice%2FB.png', stem: 'Alice%2FB' },
        { avatar: ' Alice.png', stem: ' Alice' },
        { avatar: '名字.带.点.png', stem: '名字.带.点' },
        { avatar: 'Legacy\u0080.png', stem: 'Legacy\u0080' },
    ];

    for (const item of cases) {
        const character = await service.getSingleCharacter({ avatar_url: item.avatar });
        assert.equal(character.avatar, `${item.stem}.png`);
    }

    assert.deepEqual(calls, cases.map((item) => ({
        command: 'get_character',
        args: { name: item.stem },
    })));
});

test('/api/characters/get rejects URL-like and path-like avatar_url values before backend lookup', async () => {
    const service = createCharacterService({
        safeInvoke: async () => {
            throw new Error('safeInvoke should not be called');
        },
    });
    const router = registerGetRoute(service);

    for (const avatarUrl of [
        'characters/Alice.png',
        '..\\Alice.png',
        'Alice.png?cache=1',
        'Alice.png#hash',
        'Alice.PNG',
        'Alice',
        'Bad\u007F.png',
    ]) {
        const response = await router.handle({
            method: 'POST',
            path: '/api/characters/get',
            url: new URL('http://localhost/api/characters/get'),
            body: { avatar_url: avatarUrl },
        });

        assert.ok(response);
        assert.equal(response.status, 400, avatarUrl);
        assert.deepEqual(await response.json(), { error: 'invalid avatar_url' });
    }
});

test('/api/characters/get returns 404 for exact avatar misses without falling back to character name', async () => {
    const calls = [];
    const service = createCharacterService({
        safeInvoke: async (command, args) => {
            calls.push({ command, args });
            if (command === 'get_character' && args?.name === 'Missing') {
                throw new Error('Not found: Character not found: Missing');
            }
            throw new Error(`Unexpected backend call: ${command}`);
        },
    });
    const router = registerGetRoute(service);

    const response = await router.handle({
        method: 'POST',
        path: '/api/characters/get',
        url: new URL('http://localhost/api/characters/get'),
        body: { avatar_url: 'Missing.png', name: 'Alice' },
    });

    assert.ok(response);
    assert.equal(response.status, 404);
    assert.deepEqual(await response.json(), { error: 'Character not found' });
    assert.deepEqual(calls, [
        { command: 'get_character', args: { name: 'Missing' } },
    ]);
});

test('/api/characters/get keeps name lookup only when avatar identity is absent', async () => {
    const calls = [];
    const service = createCharacterService({
        safeInvoke: async (command, args) => {
            calls.push({ command, args });
            assert.equal(command, 'get_character');
            assert.deepEqual(args, { name: 'Alice' });
            return {
                name: 'Alice',
                avatar: 'Alice.png',
                data: { extensions: {} },
            };
        },
    });

    const character = await service.getSingleCharacter({ ch_name: 'Alice' });

    assert.equal(character.avatar, 'Alice.png');
    assert.deepEqual(calls, [
        { command: 'get_character', args: { name: 'Alice' } },
    ]);
});

test('/api/characters/chats maps cached backend summaries to upstream listing shape', async () => {
    const calls = [];
    const safeInvoke = async (command, args) => {
        calls.push({ command, args });
        assert.equal(command, 'get_character_chats_by_id');
        assert.deepEqual(args, { dto: { name: 'Alice', simple: false } });
        return [{
            file_name: 'chat-a.jsonl',
            file_size: '20.00kb',
            chat_items: 3,
            last_message: 'cached preview',
            last_message_date: 1767225600000,
        }];
    };
    const service = createCharacterService({
        safeInvoke,
    });
    const router = registerGetRoute({
        ...service,
        safeInvoke,
        ensureJsonl,
        stripJsonl,
    });

    const response = await router.handle({
        method: 'POST',
        path: '/api/characters/chats',
        url: new URL('http://localhost/api/characters/chats'),
        body: { avatar_url: 'Alice.png' },
    });

    assert.ok(response);
    assert.equal(response.status, 200);
    assert.deepEqual(await response.json(), [{
        file_id: 'chat-a',
        file_name: 'chat-a.jsonl',
        file_size: '20.00kb',
        chat_items: 3,
        message_count: 3,
        mes: 'cached preview',
        last_message: 'cached preview',
        preview_message: 'cached preview',
        last_mes: 1767225600000,
    }]);
    assert.equal(calls.length, 1);
});

test('/api/characters/chats preserves simple listing request', async () => {
    const calls = [];
    const safeInvoke = async (command, args) => {
        calls.push({ command, args });
        assert.equal(command, 'get_character_chats_by_id');
        assert.deepEqual(args, { dto: { name: 'Alice', simple: true } });
        return [{
            file_name: 'chat-a.jsonl',
            file_size: '',
            chat_items: 0,
            last_message: '',
            last_message_date: 0,
        }];
    };
    const service = createCharacterService({
        safeInvoke,
    });
    const router = registerGetRoute({
        ...service,
        safeInvoke,
        ensureJsonl,
        stripJsonl,
    });

    const response = await router.handle({
        method: 'POST',
        path: '/api/characters/chats',
        url: new URL('http://localhost/api/characters/chats'),
        body: { avatar_url: 'Alice.png', simple: true },
    });

    assert.ok(response);
    assert.equal(response.status, 200);
    assert.deepEqual(await response.json(), [{
        file_id: 'chat-a',
        file_name: 'chat-a.jsonl',
        file_size: '',
        chat_items: 0,
        message_count: 0,
        mes: '',
        last_message: '',
        preview_message: '',
        last_mes: 0,
    }]);
    assert.equal(calls.length, 1);
});

test('character identity resolver treats avatar values as exact filenames without URL fallback', async () => {
    const calls = [];
    const service = createCharacterService({
        safeInvoke: async (command, args) => {
            calls.push({ command, args });
            throw new Error('safeInvoke should not be called for exact avatar identities');
        },
    });

    assert.equal(await service.resolveCharacterId({
        avatar: 'Alice#1.png',
        fallbackName: 'Alice',
    }), 'Alice#1');
    assert.equal(await service.resolveCharacterId({
        avatar: 'Alice%2FB.png',
        fallbackName: 'Alice',
    }), 'Alice%2FB');

    await assert.rejects(
        service.resolveCharacterId({
            avatar: 'Alice.png?cache=1',
            fallbackName: 'Alice',
        }),
        /Bad request: invalid avatar_url/,
    );
    assert.deepEqual(calls, []);
});

test('existing character resolver verifies exact avatar identities directly', async () => {
    const calls = [];
    const service = createCharacterService({
        safeInvoke: async (command, args) => {
            calls.push({ command, args });
            assert.equal(command, 'get_character');
            return {
                name: 'Alice',
                avatar: `${args.name}.png`,
                data: { extensions: {} },
            };
        },
    });

    assert.equal(await service.resolveExistingCharacterId({
        avatar: 'Alice%2FB.png',
        fallbackName: 'Alice',
    }), 'Alice%2FB');
    assert.deepEqual(calls, [
        { command: 'get_character', args: { name: 'Alice%2FB' } },
    ]);
});
