import test from 'node:test';
import assert from 'node:assert/strict';
import { jsonResponse } from '../src/tauri/main/http-utils.js';
import { createRouteRegistry } from '../src/tauri/main/router.js';
import { registerSettingsRoutes } from '../src/tauri/main/routes/settings-routes.js';

const revision = { token: 'loaded' };

test('settings saves preserve the text and pass only an explicit revision to the host', async () => {
    for (const expected of [null, revision]) {
        const router = createRouteRegistry();
        const text = '{ "extension_settings": { "history": "🍀世界书" } }';
        const chunks = [];
        let size = 0;
        registerSettingsRoutes(router, {
            invokeTransport: async (command, args) => {
                switch (command) {
                    case 'begin_settings_commit':
                        assert.deepEqual(args.expectedRevision, expected);
                        return { sessionId: 'session', maxFrameBytes: 1024 };
                    case 'append_settings_commit_chunk':
                        chunks.push(args);
                        size += args.byteLength;
                        return size;
                    case 'finish_settings_commit':
                        return { result: 'ok', tauritavern_settings_revision: revision };
                    default: throw new Error(command);
                }
            },
        }, { jsonResponse });
        await router.handle({
            method: 'POST', path: '/api/settings/save', body: text,
            init: { headers: expected ? { 'X-TauriTavern-Settings-Revision': JSON.stringify(expected) } : {} },
        });
        assert.equal(Buffer.concat(chunks).toString(), text);
        const invalid = await router.handle({
            method: 'POST', path: '/api/settings/save', body: text,
            init: { headers: { 'X-TauriTavern-Settings-Revision': 'null' } },
        });
        assert.equal(invalid.status, 400);
    }
});
