import assert from 'node:assert/strict';
import test from 'node:test';

import { Window } from 'happy-dom';

import { readRequestBody } from '../src/tauri/main/http-utils.js';

test('request body decoding preserves multipart data across realms', async () => {
    const hostWindow = new Window({ url: 'https://tauritavern.local/' });
    const iframe = hostWindow.document.createElement('iframe');
    hostWindow.document.body.append(iframe);
    const iframeWindow = iframe.contentWindow;
    const originalFormData = globalThis.FormData;

    globalThis.FormData = hostWindow.FormData;

    try {
        // happy-dom omits the Web IDL class string exposed by browser FormData objects.
        Object.defineProperty(iframeWindow.FormData.prototype, Symbol.toStringTag, {
            configurable: true,
            value: 'FormData',
        });

        const body = new iframeWindow.FormData();
        body.append('tag', 'first');
        body.append('avatar', new iframeWindow.File(['avatar-bytes'], 'avatar.png', {
            type: 'image/png',
        }));
        body.append('tag', 'second');

        assert.equal(body instanceof hostWindow.FormData, false);

        const decoded = await readRequestBody(null, { body });
        assert.ok(decoded instanceof hostWindow.FormData);
        assert.deepEqual(
            [...decoded.entries()].map(([name, value]) => [
                name,
                typeof value === 'string' ? value : value.name,
            ]),
            [
                ['tag', 'first'],
                ['avatar', 'avatar.png'],
                ['tag', 'second'],
            ],
        );

        const avatar = decoded.get('avatar');
        assert.ok(avatar instanceof hostWindow.File);
        assert.equal(avatar.name, 'avatar.png');
        assert.equal(avatar.type, 'image/png');
        assert.equal(await avatar.text(), 'avatar-bytes');
    } finally {
        globalThis.FormData = originalFormData;
        hostWindow.close();
    }
});
