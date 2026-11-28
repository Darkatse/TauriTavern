import { expect, test } from '@rstest/core';
import { frontendText, importFrontend } from '../helpers/frontend.mjs';

const jquerySource = await frontendText('lib/jquery-3.5.1.min.js');
const { createInterceptors } = await importFrontend('tauri/main/interceptors.js');
const { textResponse } = await importFrontend('tauri/main/http-utils.js');

function installAjax(routeRequest, options = { dataType: 'json' }) {
    const iframe = document.createElement('iframe');
    document.body.append(iframe);
    const frame = iframe.contentWindow;
    frame.eval(jquerySource);
    createInterceptors({
        originalFetch: frame.fetch.bind(frame),
        canHandleRequest: () => true,
        toUrl: (input, base) => new URL(String(input), base),
        routeRequest,
    }).patchJQueryAjax(frame);
    return frame.jQuery.ajax({ url: '/api/test', ...options });
}

function completed(xhr) {
    return new Promise((resolve, reject) => xhr.done(resolve).fail((_, status, error) => reject(error)));
}

test('jQuery abort cancels an active response body', async () => {
    const reading = Promise.withResolvers();
    const cancelled = Promise.withResolvers();
    const xhr = installAjax(async () => new Response(new ReadableStream({
        pull() { reading.resolve(); },
        cancel() { cancelled.resolve(); },
    })));
    const outcome = completed(xhr);
    await reading.promise;
    xhr.abort();
    await expect(outcome).rejects.toMatchObject({ name: 'AbortError' });
    await cancelled.promise;
});

test('jQuery accepts text responses when dataType is omitted', async () => {
    const xhr = installAjax(async () => textResponse('OK'), {});
    expect(await completed(xhr)).toBe('OK');
});
