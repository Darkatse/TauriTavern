import { expect, test } from '@rstest/core';
import { importFrontend } from '../helpers/frontend.mjs';

const { createInterceptors } = await importFrontend('tauri/main/interceptors.js');

function patchFrameFetch(routeRequest) {
    const iframe = document.createElement('iframe');
    document.body.append(iframe);
    const frame = iframe.contentWindow;
    const delegated = [];
    frame.fetch = async (...args) => {
        delegated.push(args);
        return new Response('delegated');
    };
    createInterceptors({
        originalFetch: frame.fetch.bind(frame),
        canHandleRequest: () => true,
        toUrl: (input, base) => new URL(String(input), base),
        routeRequest,
    }).patchFetch(frame);
    return { fetch: frame.fetch, delegated };
}

test('fetch interceptor rejects immediately when AbortSignal is already aborted', async () => {
    let routedCalls = 0;
    const { fetch, delegated } = patchFrameFetch(async () => {
        routedCalls += 1;
        return new Response('routed');
    });
    const controller = new AbortController();
    controller.abort();

    await expect(fetch('/api/backends/chat-completions/generate', { signal: controller.signal }))
        .rejects.toMatchObject({ name: 'AbortError' });
    expect(delegated).toEqual([]);
    expect(routedCalls).toBe(0);
});

test('fetch interceptor rejects when AbortSignal is aborted while routed request is pending', async () => {
    const route = Promise.withResolvers();
    let routedCalls = 0;
    const { fetch, delegated } = patchFrameFetch(async () => {
        routedCalls += 1;
        return route.promise;
    });
    const controller = new AbortController();
    const pending = fetch('/api/backends/chat-completions/generate', { signal: controller.signal });
    controller.abort();

    await expect(pending).rejects.toMatchObject({ name: 'AbortError' });
    const cancelled = Promise.withResolvers();
    route.resolve(new Response(new ReadableStream({ cancel() { cancelled.resolve(); } })));
    await cancelled.promise;
    expect(delegated).toEqual([]);
    expect(routedCalls).toBe(1);
});

test('fetch abort remains active after headers while a consumer reads the body', async () => {
    const reading = Promise.withResolvers();
    const cancelled = Promise.withResolvers();
    const { fetch } = patchFrameFetch(async () => new Response(new ReadableStream({
        pull() { reading.resolve(); },
        cancel() { cancelled.resolve(); },
    })));
    const controller = new AbortController();
    const response = await fetch('/api/chats/get', { signal: controller.signal });
    const body = response.json();
    await reading.promise;
    await expect(response.text()).rejects.toThrow(TypeError);
    controller.abort();

    await expect(body).rejects.toMatchObject({ name: 'AbortError' });
    await cancelled.promise;
});
