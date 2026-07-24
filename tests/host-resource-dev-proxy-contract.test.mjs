import test from 'node:test';
import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import path from 'node:path';
import vm from 'node:vm';
import { fileURLToPath } from 'node:url';

const REPO_ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');

async function readRepoFile(relativePath) {
    return readFile(path.join(REPO_ROOT, relativePath), 'utf8');
}

test('dev service worker proxies the same browser host resources as production', async () => {
    const [sw, init, endpoint, devServer] = await Promise.all([
        readRepoFile('src/tt-ext-sw.js'),
        readRepoFile('src/init.js'),
        readRepoFile('src-tauri/crates/tauritavern/src/presentation/web_resources/dev_protocol_endpoint.rs'),
        readRepoFile('scripts/tauri-dev-server.mjs'),
    ]);

    for (const route of [
        '/css/user.css',
        '/thumbnail',
        '/scripts/extensions/third-party/',
        '/characters/',
        '/backgrounds/',
        '/assets/',
        '/user/images/',
        '/user/files/',
        '/User Avatars/',
        '/User%20Avatars/',
    ]) {
        assert.ok(sw.includes(route), `tt-ext-sw.js must proxy ${route}`);
        assert.ok(init.includes(route), `init.js fallback bridge must allow ${route}`);
    }

    assert.match(init, /prefix\.endsWith\('\/'\) && pathname\.startsWith\(prefix\)/);
    for (const header of ['range', 'if-range', 'if-none-match', 'if-modified-since']) {
        assert.ok(sw.includes(`'${header}'`), `tt-ext-sw.js must forward ${header}`);
    }
    assert.match(sw, /cache: request\.cache/);
    assert.match(sw, /headers: Array\.from\(init\.headers\.entries\(\)\)/);
    assert.match(sw, /requiresIpc: requiresIpcBridge\(init\.headers\)/);
    assert.match(init, /const headers = Array\.isArray\(data\.headers\)/);
    assert.match(init, /if \(data\.requiresIpc !== true\)/);
    assert.match(init, /fetch\(targetUrl\.href/);
    assert.match(init, /installClientProxyBridge\(ttExtBaseUrl\)/);
    assert.match(init, /method,\s*\n\s*headers,/);
    assert.match(init, /protocol === 'tauri:' \|\| hostname === 'tauri\.localhost'/);
    assert.match(sw, /status === 204 \|\| status === 205 \|\| status === 304/);
    assert.match(endpoint, /ACCESS_CONTROL_EXPOSE_HEADERS/);
    assert.match(endpoint, /HeaderValue::from_static\("\*"\)/);
    assert.match(devServer, /<script src="\/dev-sw-bootstrap\.js"><\/script>/);
    assert.match(devServer, /headIndex \+ headTag\.length/);
});

test('dev bootstrap releases a service worker inherited by a new WebView session', async () => {
    const bootstrap = await readRepoFile('src/dev-sw-bootstrap.js');
    const storage = new Map();
    const unregistered = [];
    let stopped = false;
    let reloaded = false;
    const registrations = ['first', 'second'].map((name) => ({
        async unregister() {
            unregistered.push(name);
            return true;
        },
    }));
    const context = vm.createContext({
        navigator: {
            serviceWorker: {
                controller: {},
                async getRegistrations() {
                    return registrations;
                },
            },
        },
        sessionStorage: {
            getItem(key) {
                return storage.get(key) ?? null;
            },
            setItem(key, value) {
                storage.set(key, value);
            },
        },
        window: {
            location: {
                reload() {
                    reloaded = true;
                },
            },
            stop() {
                stopped = true;
            },
        },
    });

    vm.runInContext(bootstrap, context);
    await new Promise((resolve) => setImmediate(resolve));

    assert.equal(stopped, true);
    assert.equal(reloaded, true);
    assert.deepEqual(unregistered, ['first', 'second']);

    stopped = false;
    reloaded = false;
    unregistered.length = 0;
    vm.runInContext(bootstrap, context);
    await new Promise((resolve) => setImmediate(resolve));

    assert.equal(stopped, false);
    assert.equal(reloaded, false);
    assert.deepEqual(unregistered, []);
});

test('dev service worker keeps ordinary bodies on tt-ext and reserves IPC for conditionals', async () => {
    const sw = await readRepoFile('src/tt-ext-sw.js');
    const messages = [];
    const context = vm.createContext({
        URL,
        Headers,
        Response,
        MessageChannel,
        console,
        clearTimeout,
        setTimeout,
        self: {
            location: { href: 'http://localhost/tt-ext-sw.js' },
            addEventListener() {},
        },
    });
    vm.runInContext(sw, context);

    const client = {
        postMessage(message, ports) {
            messages.push(message);
            ports[0].postMessage({
                ok: true,
                status: 200,
                statusText: 'OK',
                headers: [],
                body: new ArrayBuffer(0),
            });
        },
    };
    const requestUrl = new URL('http://localhost/backgrounds/a.mp4?v=1');

    await context.sendProxyRequestToClient(client, requestUrl, {
        method: 'GET',
        cache: 'reload',
        headers: new Headers({ range: 'bytes=0-1' }),
    }, new Error('proxy failed'));
    await context.sendProxyRequestToClient(client, requestUrl, {
        method: 'GET',
        cache: 'no-cache',
        headers: new Headers({ 'if-none-match': 'W/"revision"' }),
    }, new Error('proxy failed'));

    assert.equal(messages[0].requiresIpc, false);
    assert.equal(messages[0].cache, 'reload');
    assert.deepEqual(Array.from(messages[0].headers, (entry) => Array.from(entry)), [['range', 'bytes=0-1']]);
    assert.equal(messages[1].requiresIpc, true);
    assert.equal(messages[1].cache, 'no-cache');
    assert.deepEqual(Array.from(messages[1].headers, (entry) => Array.from(entry)), [['if-none-match', 'W/"revision"']]);
});

test('dev IPC proxy respects Fetch null-body status contract', async () => {
    const sw = await readRepoFile('src/tt-ext-sw.js');
    const context = vm.createContext({
        URL,
        Headers,
        Response,
        console,
        clearTimeout,
        setTimeout,
        self: {
            location: { href: 'http://localhost/tt-ext-sw.js' },
            addEventListener() {},
        },
    });
    vm.runInContext(sw, context);

    for (const status of [204, 205, 304]) {
        const response = context.responseFromProxyPayload({
            status,
            statusText: '',
            headers: [],
            body: new ArrayBuffer(0),
        });
        assert.equal(response.status, status);
        assert.equal(response.body, null);
    }
});

test('dev service worker preserves cache mode and range on the direct protocol path', async () => {
    const sw = await readRepoFile('src/tt-ext-sw.js');
    let forwarded;
    const context = vm.createContext({
        URL,
        Headers,
        Response,
        console,
        clearTimeout,
        setTimeout,
        fetch: async (url, init) => {
            forwarded = { url, init };
            return new Response('ok', { status: 200 });
        },
        self: {
            location: { href: 'http://localhost/tt-ext-sw.js' },
            addEventListener() {},
        },
    });
    vm.runInContext(sw, context);

    const requestUrl = new URL('http://localhost/backgrounds/a.mp4?v=1');
    const request = {
        method: 'GET',
        cache: 'reload',
        headers: new Headers({ range: 'bytes=1-2' }),
    };
    const response = await context.proxyWebAssetRequest({ request }, requestUrl);

    assert.equal(response.status, 200);
    assert.equal(forwarded.url, 'tt-ext://localhost/backgrounds/a.mp4?v=1');
    assert.equal(forwarded.init.cache, 'reload');
    assert.equal(forwarded.init.headers.get('range'), 'bytes=1-2');
});

test('dev service worker sends conditional requests through IPC and preserves a bodyless 304', async () => {
    const sw = await readRepoFile('src/tt-ext-sw.js');
    let forwarded;
    const context = vm.createContext({
        URL,
        Headers,
        Response,
        console,
        clearTimeout,
        setTimeout,
        fetch: async () => {
            throw new Error('conditional request must not use the custom-protocol fetch path');
        },
        self: {
            location: { href: 'http://localhost/tt-ext-sw.js' },
            addEventListener() {},
        },
    });
    vm.runInContext(sw, context);
    context.proxyViaClientBridge = async (_event, _requestUrl, init) => {
        forwarded = init;
        return new Response(null, {
            status: 304,
            headers: { etag: 'W/"revision"' },
        });
    };

    const requestUrl = new URL('http://localhost/characters/a.png');
    const request = {
        method: 'GET',
        cache: 'no-cache',
        headers: new Headers([
            ['range', 'bytes=1-2'],
            ['if-range', '"range-revision"'],
            ['if-none-match', 'W/"revision"'],
            ['if-modified-since', 'Tue, 15 Nov 1994 12:45:26 GMT'],
        ]),
    };
    const response = await context.proxyWebAssetRequest({ request }, requestUrl);

    assert.equal(forwarded.cache, 'no-cache');
    assert.equal(forwarded.headers.get('range'), 'bytes=1-2');
    assert.equal(forwarded.headers.get('if-range'), '"range-revision"');
    assert.equal(forwarded.headers.get('if-none-match'), 'W/"revision"');
    assert.equal(
        forwarded.headers.get('if-modified-since'),
        'Tue, 15 Nov 1994 12:45:26 GMT',
    );
    assert.equal(response.status, 304);
    assert.equal(response.body, null);
    assert.equal(response.headers.get('etag'), 'W/"revision"');
});
