import test from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import path from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';

import { installFakeDom } from './helpers/fake-dom.mjs';

const REPO_ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');

async function importFresh(modulePath) {
    const url = `${pathToFileURL(modulePath).href}?t=${Date.now()}-${Math.random()}`;
    return import(url);
}

test('jQuery interceptor materializes JSON responseText only when read', async () => {
    const { createInterceptors } = await importFresh(
        path.join(REPO_ROOT, 'src/tauri/main/interceptors.js'),
    );
    const dom = installFakeDom();

    try {
        dom.window.eval(readFileSync(path.join(REPO_ROOT, 'src/lib/jquery-3.5.1.min.js'), 'utf8'));

        let serializationCount = 0;
        const payload = {
            value: 'ok',
            toJSON() {
                serializationCount += 1;
                return { value: this.value };
            },
        };
        const interceptors = createInterceptors({
            isTauri: true,
            originalFetch: dom.window.fetch.bind(dom.window),
            canHandleRequest: () => true,
            toUrl: (input, base) => new URL(String(input), base),
            routeRequest: async () => new Response('{}'),
            jsonResponse: (body, status) => new Response(JSON.stringify(body), { status }),
            safeJson: async () => payload,
        });

        interceptors.patchJQueryAjax(dom.window);

        const jqXHR = dom.window.jQuery.ajax({ url: '/api/test', dataType: 'json' });
        const resolvedPayload = await new Promise((resolve, reject) => {
            jqXHR.done(resolve).fail(reject);
        });

        assert.strictEqual(resolvedPayload, payload);
        assert.strictEqual(jqXHR.responseJSON, payload);
        assert.equal('responseText' in jqXHR, true);
        assert.equal(serializationCount, 0);
        assert.equal(jqXHR.responseText, '{"value":"ok"}');
        assert.equal(jqXHR.responseText, '{"value":"ok"}');
        assert.equal(serializationCount, 1);
    } finally {
        dom.cleanup();
    }
});
