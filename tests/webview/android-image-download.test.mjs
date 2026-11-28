import { expect, test } from '@rstest/core';
import { importFrontend } from '../helpers/frontend.mjs';

const { createDownloadBridge } = await importFrontend('tauri/main/download-bridge.js');

function createFrame() {
    const iframe = document.createElement('iframe');
    document.body.append(iframe);
    return iframe.contentWindow;
}

test('an inherited-origin window saves remote images and its own images and download links', async () => {
    const child = createFrame();
    const remoteUrl = 'https://img.invalid/100%';
    const localUrl = new URL('/get?id=portrait', location.origin).href;
    const remoteDownloads = [];
    const pageDownloads = [];
    const pageReads = [];
    const delivered = Promise.withResolvers();
    const record = (list, entry) => {
        list.push(entry);
        if (remoteDownloads.length + pageDownloads.length === 3) delivered.resolve();
        return { delivered: true };
    };
    const bridge = createDownloadBridge({
        confirmImageDownload: async () => true,
        deliverRemoteFile: async (url, fileNameFor) => record(remoteDownloads, { url, fileName: fileNameFor('image/png') }),
        deliverBlob: async (blob, fileName) => record(pageDownloads, { blob, fileName }),
    });
    child.fetch = async (url) => {
        pageReads.push(url);
        return { ok: true, blob: async () => new child.Blob(['image'], { type: 'image/png' }) };
    };
    bridge.patchWindow(child);

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
    await delivered.promise;

    expect(remoteDownloads).toEqual([{ url: remoteUrl, fileName: expect.stringMatching(/\.png$/) }]);
    expect(pageReads).toEqual([localUrl, localUrl]);
    expect(pageDownloads.map(({ fileName }) => fileName)).toContain('portrait.png');
});

test.each(['stopPropagation', 'preventDefault'])('only preventDefault cancels the image download default action: %s', async (method) => {
    const frame = createFrame();
    const confirmations = [];
    const delivered = Promise.withResolvers();
    const bridge = createDownloadBridge({
        deliverBlob: async (blob, fileName) => {
            delivered.resolve({ blob, fileName });
            return { delivered: true };
        },
        confirmImageDownload: async (source) => {
            confirmations.push(source);
            return true;
        },
    });
    bridge.patchWindow(frame);
    const blob = new frame.Blob(['image'], { type: 'image/png' });
    const image = frame.document.createElement('img');
    image.src = frame.URL.createObjectURL(blob);
    frame.document.body.append(image);
    image.addEventListener('contextmenu', (event) => event[method]());

    image.dispatchEvent(new frame.MouseEvent('contextmenu', { bubbles: true, cancelable: true }));
    // The bridge reads the outcome in a task it queued during dispatch, which runs before this one.
    await new Promise((resolve) => frame.setTimeout(resolve, 0));

    if (method === 'preventDefault') {
        expect(confirmations).toEqual([]);
        return;
    }
    expect(confirmations).toHaveLength(1);
    const download = await delivered.promise;
    expect(download.blob).toBe(blob);
    expect(download.fileName).toMatch(/\.png$/);
});
