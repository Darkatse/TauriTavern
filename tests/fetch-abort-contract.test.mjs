import test from 'node:test';
import assert from 'node:assert/strict';
import path from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';

import { installFakeDom } from './helpers/fake-dom.mjs';

const REPO_ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');

async function importFresh(modulePath) {
    const url = `${pathToFileURL(modulePath).href}?t=${Date.now()}-${Math.random()}`;
    return import(url);
}

test('fetch interceptor rejects immediately when AbortSignal is already aborted', async () => {
    const { createInterceptors } = await importFresh(
        path.join(REPO_ROOT, 'src/tauri/main/interceptors.js'),
    );

    const dom = installFakeDom();
    dom.window.location = {
        href: 'https://example.com/',
        origin: 'https://example.com',
    };

    let delegatedCalls = 0;
    dom.window.fetch = async () => {
        delegatedCalls += 1;
        return new Response('delegated');
    };

    let routedCalls = 0;
    const interceptors = createInterceptors({
        isTauri: true,
        originalFetch: dom.window.fetch.bind(dom.window),
        canHandleRequest: () => true,
        toUrl: (input, base) => new URL(String(input), base),
        routeRequest: async () => {
            routedCalls += 1;
            return new Response('routed');
        },
        jsonResponse: (body, status) => new Response(JSON.stringify(body), { status }),
    });

    interceptors.patchFetch(dom.window);

    const controller = new AbortController();
    controller.abort();

    await assert.rejects(
        dom.window.fetch('/api/backends/chat-completions/generate', { signal: controller.signal }),
        (error) => {
            assert.equal(error?.name, 'AbortError');
            return true;
        },
    );

    assert.equal(delegatedCalls, 0);
    assert.equal(routedCalls, 0);
    dom.cleanup();
});

test('fetch interceptor rejects when AbortSignal is aborted while routed request is pending', async () => {
    const { createInterceptors } = await importFresh(
        path.join(REPO_ROOT, 'src/tauri/main/interceptors.js'),
    );

    const dom = installFakeDom();
    dom.window.location = {
        href: 'https://example.com/',
        origin: 'https://example.com',
    };

    let delegatedCalls = 0;
    dom.window.fetch = async () => {
        delegatedCalls += 1;
        return new Response('delegated');
    };

    let resolveRoute = null;
    const routePromise = new Promise((resolve) => {
        resolveRoute = resolve;
    });

    let routedCalls = 0;
    const interceptors = createInterceptors({
        isTauri: true,
        originalFetch: dom.window.fetch.bind(dom.window),
        canHandleRequest: () => true,
        toUrl: (input, base) => new URL(String(input), base),
        routeRequest: async () => {
            routedCalls += 1;
            return routePromise;
        },
        jsonResponse: (body, status) => new Response(JSON.stringify(body), { status }),
    });

    interceptors.patchFetch(dom.window);

    const controller = new AbortController();
    const fetchPromise = dom.window.fetch('/api/backends/chat-completions/generate', { signal: controller.signal });
    controller.abort();

    await assert.rejects(
        fetchPromise,
        (error) => {
            assert.equal(error?.name, 'AbortError');
            return true;
        },
    );

    const cancelled = Promise.withResolvers();
    resolveRoute?.(new Response(new ReadableStream({ cancel() { cancelled.resolve(); } })));
    await cancelled.promise;

    assert.equal(delegatedCalls, 0);
    assert.equal(routedCalls, 1);
    dom.cleanup();
});

test('fetch abort remains active after headers while a consumer reads the body', async () => {
    const { createInterceptors } = await import('../src/tauri/main/interceptors.js');
    const dom = installFakeDom();
    try {
        const reading = Promise.withResolvers();
        const cancelled = Promise.withResolvers();
        createInterceptors({
            isTauri: true,
            originalFetch: dom.window.fetch.bind(dom.window),
            canHandleRequest: () => true,
            toUrl: (input, base) => new URL(String(input), base),
            routeRequest: async () => new Response(new ReadableStream({
                pull() { reading.resolve(); },
                cancel() { cancelled.resolve(); },
            })),
            jsonResponse: (body, status) => new Response(JSON.stringify(body), { status }),
        }).patchFetch(dom.window);
        const controller = new AbortController();
        const response = await dom.window.fetch('/api/chats/get', { signal: controller.signal });
        const body = response.json();
        await reading.promise;
        controller.abort();
        await assert.rejects(body, { name: 'AbortError' });
        await cancelled.promise;
    } finally {
        dom.cleanup();
    }
});
