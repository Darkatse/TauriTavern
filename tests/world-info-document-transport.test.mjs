import { installHostIdentity } from './helpers/host-identity.mjs';
import assert from 'node:assert/strict';
import test from 'node:test';

import { createRouteRegistry } from '../src/tauri/main/router.js';
import { readRequestBody, jsonResponse } from '../src/tauri/main/http-utils.js';
import { registerWorldInfoRoutes } from '../src/tauri/main/routes/worldinfo-routes.js';
import { createInvokeService } from '../src/tauri/main/services/invokes/invoke-service.js';

test.beforeEach(t => t.after(installHostIdentity()));

test('world info routes carry JSON text unchanged in both directions', async () => {
    const document = '{ "z":9007199254740993,"entries":{},"originalData":{"b":"你好 👋","a":1} }';
    const request = `{"name":" Lore ","data":${document}}`;
    const encoder = new TextEncoder();
    const frames = [];
    const service = createInvokeService({
        policies: {},
        async invoke(command, args, options) {
            switch (command) {
                case 'get_world_info': return encoder.encode(document);
                case 'begin_world_info_commit': return { sessionId: 'world', maxFrameBytes: 11 };
                case 'append_world_info_commit_chunk': {
                    frames.push(Buffer.from(args));
                    return Number(options?.headers?.offset) + args.byteLength;
                }
                case 'finish_world_info_commit': return undefined;
                default: throw new Error(`Unexpected command ${command}`);
            }
        },
    });
    const router = createRouteRegistry();
    registerWorldInfoRoutes(router, service, { jsonResponse });
    const get = await router.handle({
        method: 'POST', path: '/api/worldinfo/get', body: { name: ' Lore ' },
    });
    assert.equal(await get.text(), document);

    // Only the selected view belongs to the request body.
    const padded = encoder.encode(`!${request}!`);
    const mode = router.bodyMode('POST', '/api/worldinfo/edit');
    const body = await readRequestBody(null, { body: padded.subarray(1, -1) }, mode);
    const saved = await router.handle({ method: 'POST', path: '/api/worldinfo/edit', body });
    assert.equal(saved.status, 200);
    assert.equal(Buffer.concat(frames).toString(), request);
});
