import assert from 'node:assert/strict';
import test from 'node:test';

import { jsonResponse } from '../src/tauri/main/http-utils.js';
import { createRouteRegistry } from '../src/tauri/main/router.js';
import { createWorldInfoBroker } from '../src/tauri/main/brokers/world-info-broker.js';
import { registerWorldInfoRoutes } from '../src/tauri/main/routes/worldinfo-routes.js';
import { payloadToCreateCharacterDto } from '../src/tauri/main/services/characters/character-create-mapper.js';

test('world info get-batch preserves exact spaced names', async () => {
    const router = createRouteRegistry();
    const calls = [];
    const context = {
        async safeInvoke(command, payload) {
            calls.push({ command, payload });
            return {
                items: payload.dto.names.map((name) => ({ name, data: { entries: {} } })),
            };
        },
    };

    registerWorldInfoRoutes(router, context, { jsonResponse });

    const response = await router.handle({
        method: 'POST',
        path: '/api/worldinfo/get-batch',
        url: new URL('http://localhost/api/worldinfo/get-batch'),
        body: { names: ['Lore', ' Lore', 'Lore ', ' Lore', ''] },
    });

    assert.ok(response);
    assert.equal(response.status, 200);
    assert.deepEqual(calls[0].payload.dto.names, ['Lore', ' Lore', 'Lore ']);
    assert.deepEqual(await response.json(), {
        items: [
            { name: 'Lore', data: { entries: {} } },
            { name: ' Lore', data: { entries: {} } },
            { name: 'Lore ', data: { entries: {} } },
        ],
    });
});

test('world info broker keeps exact names as cache keys', async () => {
    let invokedNames = null;
    const broker = createWorldInfoBroker({
        flushIntervalMs: 0,
        context: {
            async safeInvoke(_command, { dto }) {
                invokedNames = dto.names;
                return {
                    items: dto.names.map((name) => ({ name, data: { entries: {}, name } })),
                };
            },
        },
    });

    const [plain, leading, trailing] = await Promise.all([
        broker.get('Lore'),
        broker.get(' Lore'),
        broker.get('Lore '),
    ]);

    assert.deepEqual(invokedNames, ['Lore', ' Lore', 'Lore ']);
    assert.equal(plain.name, 'Lore');
    assert.equal(leading.name, ' Lore');
    assert.equal(trailing.name, 'Lore ');
});

test('/api/worldinfo/sanitize-name delegates to the Rust world info naming contract', async () => {
    const router = createRouteRegistry();
    const calls = [];
    const context = {
        async safeInvoke(command, payload) {
            calls.push({ command, payload });
            return {
                name: payload.dto.import_filename ? ' Lore' : 'Lore ',
            };
        },
    };
    registerWorldInfoRoutes(router, context, { jsonResponse });

    async function sanitize(body) {
        const response = await router.handle({
            method: 'POST',
            path: '/api/worldinfo/sanitize-name',
            url: new URL('http://localhost/api/worldinfo/sanitize-name'),
            body,
        });

        assert.ok(response);
        return {
            status: response.status,
            body: await response.json(),
        };
    }

    assert.deepEqual(await sanitize({ name: 'Lore ' }), {
        status: 200,
        body: { name: 'Lore ' },
    });
    assert.deepEqual(await sanitize({ name: ' Lore.json', importFilename: true }), {
        status: 200,
        body: { name: ' Lore' },
    });
    assert.deepEqual(calls, [
        {
            command: 'normalize_world_info_name',
            payload: { dto: { name: 'Lore ', import_filename: false } },
        },
        {
            command: 'normalize_world_info_name',
            payload: { dto: { name: ' Lore.json', import_filename: true } },
        },
    ]);
});

test('character create payload preserves exact primary lorebook name', () => {
    const dto = payloadToCreateCharacterDto({
        ch_name: 'Alice',
        description: 'desc',
        first_mes: 'hello',
        world: ' Lore ',
        extensions: '{}',
    });

    assert.equal(dto.extensions.world, ' Lore ');
    assert.equal(dto.primary_lorebook, ' Lore ');
});
