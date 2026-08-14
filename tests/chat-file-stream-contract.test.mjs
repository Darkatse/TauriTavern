import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import test from 'node:test';

test('chat payload loads reuse the shared FileHandle stream on every platform', async () => {
    const source = await readFile(
        new URL('../src/scripts/tauri/chat/transport.js', import.meta.url),
        'utf8',
    );

    assert.match(source, /createReadableFileStreamService/);
    assert.equal(source.match(/createReadableFileStream\(path\)/g)?.length, 2);
    assert.doesNotMatch(source, /convertFileSrc|fetchAssetStream|isAndroidRuntime|isIosRuntime/);
    assert.doesNotMatch(source, /plugin:fs\|/);
});

test('host startup scopes the resolved data root for plugin-fs', async () => {
    const source = await readFile(
        new URL('../src-tauri/crates/tauritavern/src/app/host/resources.rs', import.meta.url),
        'utf8',
    );

    assert.match(source, /\.fs_scope\(\)/);
    assert.match(source, /allow_directory\(&runtime_paths\.data_root, true\)/);
    assert.doesNotMatch(source, /chat_staging_root/);
});
