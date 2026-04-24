import test from 'node:test';
import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const REPO_ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');

test('dev bundle exports to iOS share staging root (not downloads)', async () => {
    const devBundlePath = path.join(
        REPO_ROOT,
        'src-tauri/src/infrastructure/logging/dev_bundle.rs',
    );
    const source = await readFile(devBundlePath, 'utf8');

    const iosCfgIndex = source.indexOf('#[cfg(target_os = "ios")]');
    const notIosCfgIndex = source.indexOf('#[cfg(not(target_os = "ios"))]');
    assert.ok(iosCfgIndex >= 0, 'missing iOS cfg gate');
    assert.ok(notIosCfgIndex > iosCfgIndex, 'missing not-ios cfg gate');

    const iosSection = source.slice(iosCfgIndex, notIosCfgIndex);
    assert.match(iosSection, /fn\s+resolve_bundle_output_dir\s*\(/);
    assert.match(iosSection, /IOS_EXPORT_STAGING_ROOT_NAME/);
    assert.match(iosSection, /app_cache_dir\s*\(\s*\)/);
    assert.match(iosSection, /temp_dir\s*\(\s*\)/);
    assert.doesNotMatch(iosSection, /download_dir\s*\(/);

    const downloadDirIndex = source.search(/download_dir\s*\(/);
    assert.ok(downloadDirIndex > notIosCfgIndex, 'download_dir must be outside iOS branch');
});

test('version extension gates iOS export success toast by share completion', async () => {
    const extensionPath = path.join(
        REPO_ROOT,
        'src/scripts/extensions/tauritavern-version/index.js',
    );
    const source = await readFile(extensionPath, 'utf8');

    const shareCallIndex = source.search(
        /const\s+shareResult\s*=\s*await\s+invoke\s*\(\s*['"]ios_share_file['"]/,
    );
    assert.ok(shareCallIndex >= 0, 'missing ios_share_file invoke capture');

    const snippet = source.slice(shareCallIndex, shareCallIndex + 600);
    assert.match(snippet, /shareResult\?\.\s*completed\s*===\s*true/);
    assert.match(snippet, /toastr\?\.\s*success\?\.\s*\(/);
});

test('iOS export staging root name stays in sync across JS and Rust', async () => {
    const jsPath = path.join(REPO_ROOT, 'src/scripts/file-export.js');
    const rustPath = path.join(REPO_ROOT, 'src-tauri/src/infrastructure/paths.rs');

    const [jsSource, rustSource] = await Promise.all([
        readFile(jsPath, 'utf8'),
        readFile(rustPath, 'utf8'),
    ]);

    const jsMatch = jsSource.match(
        /IOS_EXPORT_STAGING_ROOT_NAME\s*=\s*['"]([^'"]+)['"]\s*;/,
    );
    const rustMatch = rustSource.match(
        /IOS_EXPORT_STAGING_ROOT_NAME\s*:\s*&str\s*=\s*"([^"]+)";/,
    );

    assert.ok(jsMatch, 'missing IOS_EXPORT_STAGING_ROOT_NAME in JS');
    assert.ok(rustMatch, 'missing IOS_EXPORT_STAGING_ROOT_NAME in Rust');
    assert.equal(jsMatch[1], rustMatch[1], 'staging root names diverged');
});

