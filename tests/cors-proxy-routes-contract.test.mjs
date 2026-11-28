import assert from 'node:assert/strict';
import test from 'node:test';

import { installHostIdentity } from './helpers/host-identity.mjs';
import { createRouteRegistry } from '../src/tauri/main/router.js';
import { registerCorsProxyRoutes } from '../src/tauri/main/routes/cors-proxy-routes.js';

const ORIGIN = 'http://tauri.localhost';

function createProxy(reply) {
    const calls = [];
    const router = createRouteRegistry();
    registerCorsProxyRoutes(router, {
        invokeTransport: async (command, args, options) => {
            calls.push({ command, args, options });
            return reply(command, args);
        },
    });
    const fetch = async (input, init = {}) => {
        const url = new URL(input, ORIGIN);
        return router.handle({ url, path: url.pathname, method: init.method ?? 'GET', input, init });
    };
    return { calls, fetch };
}

function sentRequest({ args, options }) {
    return {
        ...JSON.parse(decodeURIComponent(options.headers['cors-proxy-request'])),
        body: [...args],
    };
}

test('/proxy sends the page request to the target in either URL form', async (t) => {
    t.after(installHostIdentity());
    const { calls, fetch } = createProxy(() => ({ status: 204, statusText: 'No Content', headers: [], rid: null }));
    const target = 'https://api.example.com/v1/tts?format=mp3';

    for (const path of [`/proxy/${target}`, `/proxy/${encodeURIComponent(target)}`]) {
        await fetch(path, {
            method: 'POST',
            headers: { 'Content-Type': 'application/msgpack', 'X-Voice': 'alto' },
            body: new Uint8Array([0, 255, 254, 128]),
        });
    }
    const form = new FormData();
    form.append('sample', new Blob([new Uint8Array([1, 2, 3])]), 'sample.wav');
    await fetch(`/proxy/${target}`, { method: 'PUT', body: form });

    const [raw, encoded, multipart] = calls.map(sentRequest);
    assert.equal(calls[0].command, 'send_cors_proxy_request');
    assert.deepEqual(raw, encoded);
    assert.equal(raw.method, 'POST');
    assert.equal(raw.url, target);
    assert.deepEqual(raw.headers, [['content-type', 'application/msgpack'], ['x-voice', 'alto']]);
    assert.deepEqual(raw.body, [0, 255, 254, 128]);

    const contentType = new Map(multipart.headers).get('content-type');
    assert.match(contentType, /^multipart\/form-data; boundary=/);
    const sent = new Response(new Uint8Array(multipart.body), { headers: { 'Content-Type': contentType } });
    const sample = (await sent.formData()).get('sample');
    assert.deepEqual([...new Uint8Array(await sample.arrayBuffer())], [1, 2, 3]);
});

test('/proxy returns the target response with its body streamed from the host', async (t) => {
    t.after(installHostIdentity());
    const chunks = [new Uint8Array([0, 255]), new Uint8Array([128]), new Uint8Array(0)];
    const { calls, fetch } = createProxy((command) => {
        if (command === 'read_bytes') return chunks.shift().buffer;
        if (command === 'plugin:resources|close') return null;
        return { status: 201, statusText: 'Created', headers: [['content-type', 'audio/mpeg']], rid: 7 };
    });

    const response = await fetch('/proxy/https://api.example.com/v1/tts', { method: 'POST', body: '{}' });

    assert.equal(response.status, 201);
    assert.equal(response.statusText, 'Created');
    assert.equal(response.headers.get('content-type'), 'audio/mpeg');
    assert.deepEqual([...new Uint8Array(await response.arrayBuffer())], [0, 255, 128]);
    assert.deepEqual(calls.at(-1), { command: 'plugin:resources|close', args: { rid: 7 }, options: undefined });

    const { fetch: fetchEmpty } = createProxy(() => ({ status: 204, statusText: 'No Content', headers: [], rid: null }));
    assert.equal((await fetchEmpty('/proxy/https://api.example.com/v1/ping', { method: 'HEAD' })).body, null);
});
