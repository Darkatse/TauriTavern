import assert from 'node:assert/strict';
import { readFile, readdir } from 'node:fs/promises';
import path from 'node:path';
import test from 'node:test';
import { fileURLToPath } from 'node:url';

const REPO_ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');

function readProjectFile(relativePath) {
    return readFile(path.join(REPO_ROOT, relativePath), 'utf8');
}

async function firstPartyJavaScriptFiles(directory) {
    const entries = await readdir(directory, { withFileTypes: true });
    const files = [];
    for (const entry of entries) {
        const entryPath = path.join(directory, entry.name);
        if (entry.isDirectory()) {
            if (entry.name !== 'dist') {
                files.push(...await firstPartyJavaScriptFiles(entryPath));
            }
        } else if (entry.name.endsWith('.js') && !entry.name.endsWith('.min.js')) {
            files.push(entryPath);
        }
    }
    return files;
}

function customPreviewFactory(source) {
    const start = source.indexOf('async function getCustomBackgroundPreview(bg)');
    const end = source.indexOf('/**\n * Gets the new background name', start);
    assert.notEqual(start, -1, 'custom preview function is missing');
    assert.notEqual(end, -1, 'custom preview function boundary is missing');
    const implementation = source.slice(start, end);

    return new Function(
        'BACKGROUND_PREVIEW_RECIPE',
        'THUMBNAIL_CONFIG',
        'THUMBNAIL_BLOBS',
        'THUMBNAIL_STORAGE',
        'URL',
        'window',
        'Blob',
        'fetch',
        'requireOkResponse',
        'getBase64Async',
        'createThumbnail',
        `${implementation}; return getCustomBackgroundPreview;`,
    );
}

function createCustomPreviewHarness(source, sourceResponse) {
    const records = new Map();
    const blobs = new Map();
    const requests = [];
    const revoked = [];
    let nextBlobId = 0;
    class RuntimeURL extends URL {}
    RuntimeURL.createObjectURL = (blob) => {
        const url = `blob:test-${++nextBlobId}`;
        blobs.set(url, blob);
        return url;
    };
    RuntimeURL.revokeObjectURL = (url) => revoked.push(url);

    const storage = {
        async getItem(key) {
            return records.get(key) ?? null;
        },
        async setItem(key, value) {
            records.set(key, value);
        },
        async removeItem(key) {
            records.delete(key);
        },
    };
    const fetch = async (url, options = {}) => {
        requests.push({ url, method: options.method || 'GET' });
        if (String(url).startsWith('data:')) {
            return {
                ok: true,
                async blob() {
                    return new Blob(['thumbnail'], { type: 'image/png' });
                },
            };
        }
        return sourceResponse(url, options);
    };
    const memory = new Map();
    const location = new RuntimeURL('tauri://localhost/');
    const getPreview = customPreviewFactory(source)(
        'v1',
        { width: 160, height: 90 },
        memory,
        storage,
        RuntimeURL,
        { location },
        Blob,
        fetch,
        async (response) => {
            if (!response.ok) throw new Error('request failed');
        },
        async () => 'data:image/png;base64,source',
        async () => 'data:image/png;base64,thumbnail',
    );

    return { getPreview, memory, records, requests, revoked };
}

test('first-party normal reads never override Host Resource cache policy with force-cache', async () => {
    const files = await firstPartyJavaScriptFiles(path.join(REPO_ROOT, 'src'));
    const violations = [];
    for (const file of files) {
        const source = await readFile(file, 'utf8');
        if (/cache\s*:\s*['"]force-cache['"]/.test(source)) {
            violations.push(path.relative(REPO_ROOT, file));
        }
    }

    assert.deepEqual(violations, []);
});

test('background previews use stable Host Resource representations without a server Blob cache', async () => {
    const [source, settingsSource] = await Promise.all([
        readProjectFile('src/scripts/backgrounds.js'),
        readProjectFile('src/scripts/tauri/setting/setting-panel/settings-popup.js'),
    ]);

    assert.doesNotMatch(source, /SERVER_THUMBNAIL_BLOBS|getServerThumbnailBlobUrl|getThumbnailFromStorage/);
    assert.doesNotMatch(source, /cache\s*:\s*['"](?:force-cache|no-store)['"]/);
    assert.match(source, /return `\$\{getThumbnailUrl\('bg', fileName\)\}&static=true`/);
    assert.match(source, /background_settings\.animation\s*\?\s*getBackgroundPath\(bg\)\s*:\s*getStaticBackgroundThumbnailUrl\(bg\)/);
    assert.match(source, /animated && isVideoBackgroundExtension\(bg\)/);
    assert.match(source, /isVideoBackgroundExtension\(file\)[\s\S]+No-Image-Placeholder\.svg/);
    assert.match(source, /const nextUrl = generateUrlParameter\(bg, false\)/);
    assert.match(source, /background-image', 'none'[\s\S]+requestAnimationFrame[\s\S]+setBackground\(bg, nextUrl\)/);
    assert.doesNotMatch(source, /fetch\(getThumbnailUrl\('bg', bg\)[\s\S]{0,80}cache:\s*['"]reload['"]/);
    assert.match(settingsSource, /return isAnimated \? `\$\{url\}&static=true` : url/);
    assert.match(settingsSource, /endsWith\('\.mp4'\)[\s\S]+No-Image-Placeholder\.svg/);
});

test('same-origin custom previews bind candidate reuse to the Host Resource ETag', async () => {
    const source = await readProjectFile('src/scripts/backgrounds.js');
    let etag = '"source-v1"';
    const harness = createCustomPreviewHarness(source, async () => ({
        ok: true,
        headers: new Headers({ etag }),
        async blob() {
            return new Blob([etag], { type: 'image/png' });
        },
    }));
    const url = '/user/images/background.png';

    const cold = await harness.getPreview(url);
    assert.equal(harness.requests.filter(request => request.url === url && request.method === 'GET').length, 1);
    assert.equal(harness.requests.filter(request => request.url === url && request.method === 'HEAD').length, 0);
    assert.equal(harness.records.get(url).etag, '"source-v1"');

    const hot = await harness.getPreview(url);
    assert.equal(hot, cold);
    assert.equal(harness.requests.filter(request => request.url === url && request.method === 'HEAD').length, 1);
    assert.equal(harness.requests.filter(request => request.url === url && request.method === 'GET').length, 1);

    etag = '"source-v2"';
    const changed = await harness.getPreview(url);
    assert.notEqual(changed, cold);
    assert.equal(harness.requests.filter(request => request.url === url && request.method === 'HEAD').length, 2);
    assert.equal(harness.requests.filter(request => request.url === url && request.method === 'GET').length, 2);
    assert.equal(harness.records.get(url).etag, '"source-v2"');
    assert.deepEqual(harness.revoked, [cold]);
});

test('same-origin custom preview fails fast when the Host Resource omits ETag', async () => {
    const source = await readProjectFile('src/scripts/backgrounds.js');
    const harness = createCustomPreviewHarness(source, async () => ({
        ok: true,
        headers: new Headers(),
        async blob() {
            throw new Error('body must not be read without a validator');
        },
    }));

    await assert.rejects(
        harness.getPreview('/user/images/background.png'),
        /missing ETag/,
    );
    assert.equal(harness.memory.size, 0);
    assert.equal(harness.records.size, 0);
});

test('persona paths are stable, exactly-once encoded, and re-demanded through the DOM', async () => {
    const [personaSource, scriptSource] = await Promise.all([
        readProjectFile('src/scripts/personas.js'),
        readProjectFile('src/script.js'),
    ]);

    assert.match(
        personaSource,
        /export function getUserAvatar\(avatarImg\) \{\s*return `\$\{USER_AVATAR_PATH\}\$\{encodeURIComponent\(String\(avatarImg \?\? ''\)\)\}`;\s*\}/,
    );
    assert.doesNotMatch(personaSource, /forFetch|__TAURITAVERN_PERSONA_PATH__|cache\s*:\s*['"]reload['"]/);
    assert.doesNotMatch(personaSource, /getThumbnailUrl\('persona', avatarId,/);
    assert.match(personaSource, /avatarImages\.attr\('src', ''\)[\s\S]+requestAnimationFrame[\s\S]+getThumbnailUrl\('persona', user_avatar\)/);
    assert.match(scriptSource, /new URL\(thumbURL, window\.location\.href\)\.searchParams\.get\('file'\)/);
    assert.match(scriptSource, /charsPath \+ encodeURIComponent\(targetAvatarImg\)/);
    assert.doesNotMatch(scriptSource, /decodeURIComponent\(targetAvatarImg\)/);
});

test('Host ABI stays v1 after removing internal cache bypasses', async () => {
    const [bootstrap, context, types, upstreamFacade, frontendCommands, rustRegistry] = await Promise.all([
        readProjectFile('src/tauri/main/bootstrap.js'),
        readProjectFile('src/tauri/main/context/index.js'),
        readProjectFile('src/types.d.ts'),
        readProjectFile('src/script.js'),
        readProjectFile('src/tauri/main/kernel/invokes/tauri-commands.js'),
        readProjectFile('src-tauri/crates/tauritavern/src/presentation/commands/registry.rs'),
    ]);

    assert.match(bootstrap, /const HOST_ABI_VERSION = 1;/);
    for (const source of [bootstrap, context, types]) {
        assert.doesNotMatch(source, /thumbnailBlobUrl|THUMBNAIL_BLOB_URL|AVATAR_PATH|PERSONA_PATH/);
    }
    assert.doesNotMatch(context, /convertFileSrc|get_default_user_directory|createThumbnailService/);
    assert.match(
        upstreamFacade,
        /if \(typeof window\.__TAURITAVERN_THUMBNAIL__ === 'function'\) \{\s*return window\.__TAURITAVERN_THUMBNAIL__\(type, file, t\);\s*\}/,
    );

    for (const command of [
        'get_default_user_directory',
        'read_thumbnail_asset',
        'read_user_avatar_asset',
        'read_user_file_asset',
    ]) {
        assert.doesNotMatch(frontendCommands, new RegExp(command));
        assert.doesNotMatch(rustRegistry, new RegExp(command));
    }
});
