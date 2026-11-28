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

test('Tauri resource helpers encode stable Host Resource URLs', async () => {
    const { windowMock } = installWindowMocks();

    const { buildThumbnailUrl, installAssetPathHelpers } = await importFresh(
        path.join(REPO_ROOT, 'src/tauri/main/context/asset-path-helpers.js'),
    );

    installAssetPathHelpers();

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

    assert.equal(
        buildThumbnailUrl('avatar', 'a b.png', { list: true }),
        '/thumbnail?type=avatar&list=true&file=a+b.png',
    );
    assert.equal(
        buildThumbnailUrl('bg', 'a.gif', { static: true }),
        '/thumbnail?type=bg&static=true&file=a.gif',
    );
    assert.match(thumbnailPathFn('avatar', 'a.png', true), /^\/thumbnail\?type=avatar&t=\d+&file=a\.png$/);
});
