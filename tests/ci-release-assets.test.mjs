import assert from 'node:assert/strict';
import { mkdir, mkdtemp, readdir, rm, writeFile } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { basename, dirname, join } from 'node:path';
import test from 'node:test';

import { collectReleaseAssets } from '../scripts/ci/collect-release-assets.mjs';

const ARTIFACTS = new Map([
    ['android-arm-apk', 'app-armeabi-v7a-release.apk'],
    ['android-arm64-apk', 'app-arm64-v8a-release.apk'],
    ['darwin-aarch64-app', 'TauriTavern.app/Contents/MacOS/TauriTavern'],
    ['darwin-aarch64-dmg', 'TauriTavern_aarch64.dmg'],
    ['darwin-x64-app', 'TauriTavern.app/Contents/MacOS/TauriTavern'],
    ['darwin-x64-dmg', 'TauriTavern_x64.dmg'],
    ['debug-darwin-aarch64-app', 'TauriTavern.app/Contents/MacOS/TauriTavern'],
    ['debug-darwin-aarch64-dmg', 'TauriTavern_aarch64.dmg'],
    ['debug-darwin-x64-app', 'TauriTavern.app/Contents/MacOS/TauriTavern'],
    ['debug-darwin-x64-dmg', 'TauriTavern_x64.dmg'],
    ['debug-windows-x64-nsis', 'TauriTavern_2.2.0_x64-setup.exe'],
    ['ios-arm64-ipa', 'TauriTavern.ipa'],
    ['ios-arm64-TestFlight-ipa', 'TauriTavern.ipa'],
    ['linux-aarch64-rpm', 'TauriTavern-2.2.0-1.aarch64.rpm'],
    ['linux-amd64-appimage', 'TauriTavern_2.2.0_amd64.AppImage'],
    ['linux-amd64-deb', 'TauriTavern_2.2.0_amd64.deb'],
    ['linux-arm64-deb', 'TauriTavern_2.2.0_arm64.deb'],
    ['linux-arm64-portable', 'TauriTavern-linux-arm64-portable'],
    ['linux-x64-portable', 'TauriTavern-linux-x64-portable'],
    ['linux-x86_64-rpm', 'TauriTavern-2.2.0-1.x86_64.rpm'],
    ['windows-x64-msi', 'TauriTavern_2.2.0_x64_en-US.msi'],
    ['windows-x64-nsis', 'TauriTavern_2.2.0_x64-setup.exe'],
    ['windows-x64-portable', 'TauriTavern-windows-x64-portable.exe'],
]);

const EXPECTED_RELEASE_ASSETS = [
    'TauriTavern-2.2.0-android-arm64-v8a.apk',
    'TauriTavern-2.2.0-android-armeabi-v7a.apk',
    'TauriTavern-2.2.0-ios-arm64.ipa',
    'TauriTavern-2.2.0-ios-arm64-TestFlight.ipa',
    'TauriTavern-2.2.0-linux-arm64-portable',
    'TauriTavern-2.2.0-linux-arm64.deb',
    'TauriTavern-2.2.0-linux-arm64.rpm',
    'TauriTavern-2.2.0-linux-x64-portable',
    'TauriTavern-2.2.0-linux-x64.AppImage',
    'TauriTavern-2.2.0-linux-x64.deb',
    'TauriTavern-2.2.0-linux-x64.rpm',
    'TauriTavern-2.2.0-macos-arm64-DEBUG.dmg',
    'TauriTavern-2.2.0-macos-arm64.dmg',
    'TauriTavern-2.2.0-macos-x64-DEBUG.dmg',
    'TauriTavern-2.2.0-macos-x64.dmg',
    'TauriTavern-2.2.0-windows-x64-portable.exe',
    'TauriTavern-2.2.0-windows-x64-setup-DEBUG.exe',
    'TauriTavern-2.2.0-windows-x64-setup.exe',
    'TauriTavern-2.2.0-windows-x64.msi',
].sort();

async function createArtifacts(root, prefix, omittedSuffixes = []) {
    const omitted = new Set(Array.isArray(omittedSuffixes) ? omittedSuffixes : [omittedSuffixes]);
    for (const [suffix, relativeFile] of ARTIFACTS) {
        if (omitted.has(suffix)) {
            continue;
        }
        const file = join(root, `${prefix}${suffix}`, relativeFile);
        await mkdir(dirname(file), { recursive: true });
        await writeFile(file, suffix);
    }
}

test('collectReleaseAssets publishes the complete Stable naming contract', async (t) => {
    const root = await mkdtemp(join(tmpdir(), 'tauritavern-release-assets-'));
    t.after(() => rm(root, { recursive: true, force: true }));

    const inputDirectory = join(root, 'dist');
    const outputDirectory = join(root, 'release-assets');
    const artifactPrefix = 'TauriTavern-2.2.0-';
    await createArtifacts(inputDirectory, artifactPrefix);

    const assets = await collectReleaseAssets({
        inputDirectory,
        outputDirectory,
        artifactPrefix,
        requireDebug: true,
    });

    assert.deepEqual(assets.map((asset) => basename(asset)), EXPECTED_RELEASE_ASSETS);
    assert.deepEqual((await readdir(outputDirectory)).sort(), EXPECTED_RELEASE_ASSETS);
});

test('collectReleaseAssets fails before publishing an incomplete release set', async (t) => {
    const root = await mkdtemp(join(tmpdir(), 'tauritavern-release-assets-'));
    t.after(() => rm(root, { recursive: true, force: true }));

    const inputDirectory = join(root, 'dist');
    const outputDirectory = join(root, 'release-assets');
    const artifactPrefix = 'TauriTavern-20260726-canary-';
    await createArtifacts(inputDirectory, artifactPrefix, [
        'android-arm64-apk',
        'debug-darwin-aarch64-app',
        'debug-darwin-aarch64-dmg',
        'debug-darwin-x64-app',
        'debug-darwin-x64-dmg',
        'debug-windows-x64-nsis',
    ]);

    await assert.rejects(
        collectReleaseAssets({
            inputDirectory,
            outputDirectory,
            artifactPrefix,
        }),
        { message: 'Missing workflow artifacts: TauriTavern-20260726-canary-android-arm64-apk' },
    );
    await assert.rejects(readdir(outputDirectory), { code: 'ENOENT' });
});

test('collectReleaseAssets requires every Stable debug artifact', async (t) => {
    const root = await mkdtemp(join(tmpdir(), 'tauritavern-release-assets-'));
    t.after(() => rm(root, { recursive: true, force: true }));

    const inputDirectory = join(root, 'dist');
    const outputDirectory = join(root, 'release-assets');
    const artifactPrefix = 'TauriTavern-2.2.0-';
    await createArtifacts(inputDirectory, artifactPrefix, 'debug-windows-x64-nsis');

    await assert.rejects(
        collectReleaseAssets({ inputDirectory, outputDirectory, artifactPrefix, requireDebug: true }),
        /Missing workflow artifacts: TauriTavern-2\.2\.0-debug-windows-x64-nsis/,
    );
});
