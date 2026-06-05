import test from 'node:test';
import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const REPO_ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');

test('Android Blob exports use the native public Downloads bridge', async () => {
    const fileExportPath = path.join(REPO_ROOT, 'src/scripts/file-export.js');
    const source = await readFile(fileExportPath, 'utf8');

    assert.match(source, /TauriTavernAndroidPublicDownloadBridge/);
    assert.match(source, /saveBlobWithAndroidPublicDownloadRuntime/);
    assert.match(source, /saveStagedFileToAndroidPublicDownloads/);
    assert.match(source, /resolveAndroidExportStagingDirectory/);
    assert.match(source, /ANDROID_EXPORT_STAGING_ROOT_NAME/);
    assert.doesNotMatch(source, /ANDROID_APP_SCOPED_DIR_PATTERN/);
    assert.doesNotMatch(source, /resolveAndroidPublicDownloadDirectory/);
    assert.doesNotMatch(source, /writeBlobToMobileDownloadFolder/);
});

test('Android native public download bridge writes through MediaStore Downloads', async () => {
    const bridgePath = path.join(
        REPO_ROOT,
        'src-tauri/gen/android/app/src/main/java/com/tauritavern/client/AndroidPublicDownloadJsBridge.kt',
    );
    const source = await readFile(bridgePath, 'utf8');

    assert.match(source, /MediaStore\.Downloads\.EXTERNAL_CONTENT_URI/);
    assert.match(source, /MediaStore\.MediaColumns\.RELATIVE_PATH/);
    assert.match(source, /MediaStore\.MediaColumns\.IS_PENDING/);
    assert.match(source, /Environment\.DIRECTORY_DOWNLOADS/);
    assert.match(source, /exportStagingRoot/);
    assert.match(source, /canonicalFile/);
    assert.match(source, /isDescendantOf/);
});

test('Android public Downloads are not exposed as raw Tauri fs write scopes', async () => {
    const capabilityPath = path.join(REPO_ROOT, 'src-tauri/capabilities/default.json');
    const source = await readFile(capabilityPath, 'utf8');

    assert.doesNotMatch(source, /\/storage\/emulated\/0\/Download\/\*\*/);
    assert.doesNotMatch(source, /\/sdcard\/Download\/\*\*/);
});

test('Android public download staging root stays aligned across JS and Kotlin', async () => {
    const jsPath = path.join(REPO_ROOT, 'src/scripts/file-export.js');
    const bridgePath = path.join(
        REPO_ROOT,
        'src-tauri/gen/android/app/src/main/java/com/tauritavern/client/AndroidPublicDownloadJsBridge.kt',
    );

    const [jsSource, bridgeSource] = await Promise.all([
        readFile(jsPath, 'utf8'),
        readFile(bridgePath, 'utf8'),
    ]);

    const jsRootMatch = jsSource.match(
        /IOS_EXPORT_STAGING_ROOT_NAME\s*=\s*['"]([^'"]+)['"]\s*;/,
    );
    const bridgeRootMatch = bridgeSource.match(
        /EXPORT_STAGING_ROOT_NAME\s*=\s*"([^"]+)"/,
    );

    assert.ok(jsRootMatch, 'missing JS export staging root name');
    assert.ok(bridgeRootMatch, 'missing Android bridge export staging root name');
    assert.equal(jsRootMatch[1], bridgeRootMatch[1], 'Android export staging roots diverged');
});
