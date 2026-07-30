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

test('Tauri resource helpers expose only stable Host Resource URLs', async () => {
    const { windowMock } = installWindowMocks();

    const { installAssetPathHelpers } = await importFresh(
        path.join(REPO_ROOT, 'src/tauri/main/context/asset-path-helpers.js'),
    );

    installAssetPathHelpers({
        thumbnailRouteTypes: new Set(['bg', 'avatar', 'persona']),
    });

    const backgroundPathFn = windowMock.__TAURITAVERN_BACKGROUND_PATH__;
    const thumbnailPathFn = windowMock.__TAURITAVERN_THUMBNAIL__;
    assert.equal(typeof backgroundPathFn, 'function');
    assert.equal(typeof thumbnailPathFn, 'function');

    assert.equal(backgroundPathFn('test.mp4.jpg'), '/backgrounds/test.mp4.jpg');
    assert.equal(thumbnailPathFn('bg', 'test image.png'), '/thumbnail?type=bg&file=test+image.png');

    for (const file of [
        'space name.png',
        'plus+amp&hash#.png',
        'literal%percent.png',
        '雪 😀.png',
    ]) {
        assert.equal(backgroundPathFn(file), `/backgrounds/${encodeURIComponent(file)}`);
        assert.equal(
            thumbnailPathFn('bg', file),
            `/thumbnail?${new URLSearchParams({ type: 'bg', file }).toString()}`,
        );

        const parsedThumbnail = new URL(thumbnailPathFn('bg', file), 'http://localhost');
        assert.equal(parsedThumbnail.searchParams.get('file'), file);
        assert.equal(parsedThumbnail.searchParams.has('t'), false);
    }

    assert.throws(() => thumbnailPathFn('unknown', 'test.png'), /Unsupported thumbnail type/);

    assert.equal(windowMock.__TAURITAVERN_THUMBNAIL_BLOB_URL__, undefined);
    assert.equal(windowMock.__TAURITAVERN_AVATAR_PATH__, undefined);
    assert.equal(windowMock.__TAURITAVERN_PERSONA_PATH__, undefined);
});
