import test from 'node:test';
import assert from 'node:assert/strict';
import path from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';

const REPO_ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');

async function importFresh(modulePath) {
    const url = `${pathToFileURL(modulePath).href}?t=${Date.now()}-${Math.random()}`;
    return import(url);
}

function installWindowMocks() {
    const windowMock = {
        addEventListener() {},
    };

    const documentMock = {
        visibilityState: 'visible',
        addEventListener() {},
    };

    globalThis.window = windowMock;
    globalThis.document = documentMock;

    return { windowMock, documentMock };
}

test('Tauri background path helper uses /backgrounds route (not asset protocol)', async () => {
    const { windowMock } = installWindowMocks();

    const assetService = {
        buildThumbnailRouteUrl: () => '/thumbnail?type=bg&file=a',
        resolveAssetPath: () => '/storage/emulated/0/Android/data/com.tauritavern.client/data/default-user/backgrounds/a.png',
        toAssetUrl: () => 'http://asset.localhost/%2Fstorage%2Femulated%2F0%2FAndroid%2Fdata%2Fcom.tauritavern.client%2Fdata%2Fdefault-user%2Fbackgrounds%2Fa.png',
    };

    const thumbnailService = {
        async resolveThumbnailBlobUrl() {
            return 'blob:ok';
        },
    };

    const { installAssetPathHelpers } = await importFresh(
        path.join(REPO_ROOT, 'src/tauri/main/context/asset-path-helpers.js'),
    );

    installAssetPathHelpers({
        assetService,
        thumbnailService,
        thumbnailRouteTypes: new Set(['bg', 'avatar', 'persona']),
    });

    const backgroundPathFn = windowMock.__TAURITAVERN_BACKGROUND_PATH__;
    assert.equal(typeof backgroundPathFn, 'function');

    const resolved = backgroundPathFn('test.mp4.jpg');

    assert.equal(resolved, '/backgrounds/test.mp4.jpg');
});
