#!/usr/bin/env node

import { constants } from 'node:fs';
import { copyFile, mkdir, readdir } from 'node:fs/promises';
import { basename, extname, join, resolve } from 'node:path';
import { pathToFileURL } from 'node:url';

// tauri-action exposes platform-specific artifact tokens. Keep that vocabulary
// at the CI boundary and publish a stable, project-owned naming contract.
const RELEASE_ASSETS = new Map([
    ['android-arm-apk', ['android-armeabi-v7a.apk', '.apk']],
    ['android-arm64-apk', ['android-arm64-v8a.apk', '.apk']],
    ['darwin-aarch64-dmg', ['macos-arm64.dmg', '.dmg']],
    ['darwin-x64-dmg', ['macos-x64.dmg', '.dmg']],
    ['debug-darwin-aarch64-dmg', ['macos-arm64-DEBUG.dmg', '.dmg']],
    ['debug-darwin-x64-dmg', ['macos-x64-DEBUG.dmg', '.dmg']],
    ['debug-windows-x64-nsis', ['windows-x64-setup-DEBUG.exe', '.exe']],
    ['ios-arm64-ipa', ['ios-arm64.ipa', '.ipa']],
    ['ios-arm64-TestFlight-ipa', ['ios-arm64-TestFlight.ipa', '.ipa']],
    ['linux-aarch64-rpm', ['linux-arm64.rpm', '.rpm']],
    ['linux-amd64-appimage', ['linux-x64.AppImage', '.AppImage']],
    ['linux-amd64-deb', ['linux-x64.deb', '.deb']],
    ['linux-arm64-deb', ['linux-arm64.deb', '.deb']],
    ['linux-arm64-portable', ['linux-arm64-portable', '']],
    ['linux-x64-portable', ['linux-x64-portable', '']],
    ['linux-x86_64-rpm', ['linux-x64.rpm', '.rpm']],
    ['windows-x64-msi', ['windows-x64.msi', '.msi']],
    ['windows-x64-nsis', ['windows-x64-setup.exe', '.exe']],
    ['windows-x64-portable', ['windows-x64-portable.exe', '.exe']],
]);

const IGNORED_ARTIFACTS = new Set([
    'darwin-aarch64-app',
    'darwin-x64-app',
    'debug-darwin-aarch64-app',
    'debug-darwin-x64-app',
]);

const DEBUG_ARTIFACTS = new Set([
    'debug-darwin-aarch64-dmg',
    'debug-darwin-x64-dmg',
    'debug-windows-x64-nsis',
]);

async function listFiles(directory) {
    const entries = await readdir(directory, { withFileTypes: true });
    const files = [];

    for (const entry of entries) {
        const path = join(directory, entry.name);
        if (entry.isDirectory()) {
            files.push(...await listFiles(path));
        } else if (entry.isFile()) {
            files.push(path);
        } else {
            throw new Error(`Unsupported entry in workflow artifact: ${path}`);
        }
    }

    return files;
}

function validatePrefix(prefix) {
    if (!prefix || prefix === '.' || basename(prefix) !== prefix || !prefix.endsWith('-')) {
        throw new Error(`Release asset prefix must be a filename prefix ending in "-", received: ${prefix || '(empty)'}`);
    }
}

function hasExpectedExtension(path, expectedExtension) {
    if (expectedExtension) {
        return path.endsWith(expectedExtension);
    }
    return extname(path) === '';
}

export async function collectReleaseAssets({
    inputDirectory,
    outputDirectory,
    artifactPrefix,
    requireDebug = false,
}) {
    validatePrefix(artifactPrefix);

    const entries = await readdir(inputDirectory, { withFileTypes: true });
    const artifacts = entries.filter((entry) => entry.name.startsWith(artifactPrefix));
    if (artifacts.length === 0) {
        throw new Error(`No workflow artifacts found with prefix ${artifactPrefix}`);
    }

    const found = new Set();
    const copies = [];

    for (const artifact of artifacts) {
        if (!artifact.isDirectory()) {
            throw new Error(`Workflow artifact is not a directory: ${join(inputDirectory, artifact.name)}`);
        }

        const artifactSuffix = artifact.name.slice(artifactPrefix.length);
        if (IGNORED_ARTIFACTS.has(artifactSuffix)) {
            continue;
        }

        const releaseAsset = RELEASE_ASSETS.get(artifactSuffix);
        if (!releaseAsset) {
            throw new Error(`Unknown workflow artifact: ${artifact.name}`);
        }

        const [releaseSuffix, expectedExtension] = releaseAsset;
        const files = await listFiles(join(inputDirectory, artifact.name));
        if (files.length !== 1) {
            throw new Error(`Expected one file in ${artifact.name}, found ${files.length}`);
        }
        if (!hasExpectedExtension(files[0], expectedExtension)) {
            throw new Error(`Unexpected file type in ${artifact.name}: ${files[0]}`);
        }

        found.add(artifactSuffix);
        copies.push({
            source: files[0],
            destination: join(outputDirectory, `${artifactPrefix}${releaseSuffix}`),
        });
    }

    const missing = [...RELEASE_ASSETS.keys()]
        .filter((suffix) => !found.has(suffix) && (requireDebug || !DEBUG_ARTIFACTS.has(suffix)));
    if (missing.length > 0) {
        throw new Error(`Missing workflow artifacts: ${missing.map((suffix) => artifactPrefix + suffix).join(', ')}`);
    }

    const destinations = copies.map(({ destination }) => destination);
    if (new Set(destinations).size !== destinations.length) {
        throw new Error('Multiple workflow artifacts map to the same release asset');
    }

    const created = await mkdir(outputDirectory, { recursive: true });
    if (created === undefined) {
        throw new Error(`Release asset output directory already exists: ${outputDirectory}`);
    }

    for (const { source, destination } of copies) {
        await copyFile(source, destination, constants.COPYFILE_EXCL);
    }

    return destinations.sort();
}

async function main() {
    const [inputDirectory, outputDirectory, artifactPrefix, option] = process.argv.slice(2);
    if (
        !inputDirectory
        || !outputDirectory
        || !artifactPrefix
        || (option !== undefined && option !== '--require-debug')
        || process.argv.length < 5
        || process.argv.length > 6
    ) {
        throw new Error(
            'Usage: collect-release-assets.mjs <workflow-artifacts-dir> <release-assets-dir> <artifact-prefix> [--require-debug]',
        );
    }

    const assets = await collectReleaseAssets({
        inputDirectory: resolve(inputDirectory),
        outputDirectory: resolve(outputDirectory),
        artifactPrefix,
        requireDebug: option === '--require-debug',
    });
    process.stdout.write(`${assets.join('\n')}\n`);
}

if (process.argv[1] && pathToFileURL(resolve(process.argv[1])).href === import.meta.url) {
    await main();
}
