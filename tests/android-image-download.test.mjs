import assert from 'node:assert/strict';
import test from 'node:test';
import { Window } from 'happy-dom';

import { createDownloadBridge } from '../src/tauri/main/download-bridge.js';

test('only preventDefault cancels the image download default action', async (t) => {
    for (const method of ['stopPropagation', 'preventDefault']) {
        await t.test(method, async () => {
            const window = new Window({ url: 'https://tauritavern.local/' });
            const confirmations = [];
            const downloads = [];
            const bridge = createDownloadBridge({
                isNativeMobileDownloadRuntime: () => true,
                downloadBlobWithRuntime: async (blob, fileName) => {
                    downloads.push({ blob, fileName });
                },
                confirmImageDownload: async (source) => {
                    confirmations.push(source);
                    return true;
                },
            });

            try {
                bridge.patchWindow(window);
                const blob = new window.Blob(['image'], { type: 'image/png' });
                const image = window.document.createElement('img');
                image.src = window.URL.createObjectURL(blob);
                window.document.body.append(image);
                image.addEventListener('contextmenu', (event) => event[method]());

                image.dispatchEvent(new window.MouseEvent('contextmenu', {
                    bubbles: true,
                    cancelable: true,
                }));
                await window.happyDOM.waitUntilComplete();

                const expectedCount = method === 'preventDefault' ? 0 : 1;
                assert.equal(confirmations.length, expectedCount);
                assert.equal(downloads.length, expectedCount);
                if (expectedCount) {
                    assert.equal(downloads[0].blob, blob);
                    assert.ok(downloads[0].fileName.endsWith('.png'));
                }
                window.URL.revokeObjectURL(image.src);
            } finally {
                await window.happyDOM.close();
            }
        });
    }
});
