import test from 'node:test';
import assert from 'node:assert/strict';
import { createExtensionAssetLoader } from '../src/scripts/extensions/runtime/asset-loader.js';

test('extension script loading waits for top-level await evaluation', async () => {
    const originalDocument = globalThis.document;
    const marker = '__tt_extension_ready';
    const elements = new Map();
    const scriptUrl = `data:text/javascript,${encodeURIComponent(`
        await Promise.resolve();
        globalThis[${JSON.stringify(marker)}] = true;
    `)}`;

    globalThis.document = {
        getElementById: id => elements.get(id) ?? null,
        createElement: () => ({ dataset: {} }),
        body: {
            appendChild(script) {
                elements.set(script.id, script);
                queueMicrotask(() => script.onload());
            },
        },
    };

    try {
        const loader = createExtensionAssetLoader({
            sanitizeSelector: value => value.replaceAll('/', '_'),
            getExtensionResourceUrl: () => scriptUrl,
            isThirdPartyExtension: () => false,
            resolveThirdPartyStylesheetUrl: async url => url,
        });

        await loader.addExtensionScript('third-party/top-level-await', { js: 'index.js' });

        assert.equal(globalThis[marker], true);
        assert.equal(elements.get('third-party_top-level-await-js').dataset.tauritavernLoaded, 'true');
    } finally {
        delete globalThis[marker];
        if (originalDocument === undefined) {
            delete globalThis.document;
        } else {
            globalThis.document = originalDocument;
        }
    }
});
