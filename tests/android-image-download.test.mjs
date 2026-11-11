import assert from 'node:assert/strict';
import test from 'node:test';
import { Window } from 'happy-dom';

import { createDownloadBridge } from '../src/tauri/main/download-bridge.js';

test('an inherited-origin window saves remote images and its own images and download links', async () => {
    const window = new Window({ url: 'https://tauritavern.local/' });
    const iframe = window.document.createElement('iframe');
    window.document.body.append(iframe);
    const child = iframe.contentWindow;
    // happy-dom lacks window.origin; Chromium inherits it even when location.origin is 'null'.
    child.origin = window.location.origin;
    const remoteDownloads = [];
    const pageDownloads = [];
    const pageReads = [];
    const bridge = createDownloadBridge({
        confirmImageDownload: async () => true,
        deliverRemoteFile: async (url, fileNameFor) => {
            remoteDownloads.push({ url, fileName: fileNameFor('image/png') });
            return { delivered: true };
        },
        deliverBlob: async (blob, fileName) => {
            pageDownloads.push({ blob, fileName });
            return { delivered: true };
        },
    });

    try {
        child.fetch = async (url) => {
            pageReads.push(url);
            return { ok: true, blob: async () => new child.Blob(['image'], { type: 'image/png' }) };
        };
        bridge.patchWindow(child);
        const remoteUrl = 'https://img.example.com/100%';
        const localUrl = 'https://tauritavern.local/get?id=portrait';
        for (const src of [remoteUrl, localUrl]) {
            const image = child.document.createElement('img');
            image.src = src;
            child.document.body.append(image);
            image.dispatchEvent(new child.MouseEvent('contextmenu', { bubbles: true, cancelable: true }));
        }
        const anchor = child.document.createElement('a');
        anchor.href = localUrl;
        anchor.download = 'portrait.png';
        child.document.body.append(anchor);
        anchor.click();
        await window.happyDOM.waitUntilComplete();

        assert.equal(remoteDownloads.length, 1);
        assert.equal(remoteDownloads[0].url, remoteUrl);
        assert.ok(remoteDownloads[0].fileName.endsWith('.png'));
        assert.deepEqual(pageReads, [localUrl, localUrl]);
        assert.equal(pageDownloads.length, 2);
        assert.ok(pageDownloads.some(({ fileName }) => fileName === 'portrait.png'));
    } finally {
        await window.happyDOM.close();
    }
});

test('only preventDefault cancels the image download default action', async (t) => {
    for (const method of ['stopPropagation', 'preventDefault']) {
        await t.test(method, async () => {
            const window = new Window({ url: 'https://tauritavern.local/' });
            const confirmations = [];
            const downloads = [];
            const bridge = createDownloadBridge({
                deliverBlob: async (blob, fileName) => {
                    downloads.push({ blob, fileName });
                    return { delivered: true };
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
