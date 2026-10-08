import test from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { installFakeDom } from './helpers/fake-dom.mjs';
import { createInterceptors } from '../src/tauri/main/interceptors.js';
import { textResponse } from '../src/tauri/main/http-utils.js';

function installAjax(t, routeRequest, options = { dataType: 'json' }) {
    const nativeQueueMicrotask = globalThis.queueMicrotask;
    const dom = installFakeDom();
    globalThis.queueMicrotask = nativeQueueMicrotask;
    t.after(() => dom.cleanup());
    dom.window.eval(readFileSync(new URL('../src/lib/jquery-3.5.1.min.js', import.meta.url), 'utf8'));
    createInterceptors({
        isTauri: true,
        originalFetch: dom.window.fetch.bind(dom.window),
        canHandleRequest: () => true,
        toUrl: (input, base) => new URL(String(input), base),
        routeRequest,
    }).patchJQueryAjax(dom.window);
    return dom.window.jQuery.ajax({ url: '/api/test', ...options });
}

function completed(xhr) {
    return new Promise((resolve, reject) => xhr.done(resolve).fail((_, status, error) => reject(error)));
}

test('jQuery abort cancels an active response body', async t => {
    const reading = Promise.withResolvers();
    const cancelled = Promise.withResolvers();
    const xhr = installAjax(t, async () => new Response(new ReadableStream({
        pull() { reading.resolve(); },
        cancel() { cancelled.resolve(); },
    })));
    const outcome = completed(xhr);
    await reading.promise;
    xhr.abort();
    await assert.rejects(outcome, { name: 'AbortError' });
    await cancelled.promise;
});

test('jQuery accepts text responses when dataType is omitted', async t => {
    const xhr = installAjax(t, async () => textResponse('OK'), {});
    assert.equal(await completed(xhr), 'OK');
});
