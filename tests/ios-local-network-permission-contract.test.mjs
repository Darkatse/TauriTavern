import test from 'node:test';
import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const REPO_ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');

const LOCAL_NETWORK_KEY_PATTERN =
    /<key>NSLocalNetworkUsageDescription<\/key>\s*<string>[^<]+<\/string>/;

test('iOS Info.plist declares Local Network usage description (LAN Sync)', async () => {
    const plistPath = path.join(REPO_ROOT, 'src-tauri/Info.ios.plist');
    const source = await readFile(plistPath, 'utf8');

    assert.match(source, LOCAL_NETWORK_KEY_PATTERN);
    assert.match(source, /LAN Sync/i);
});

test('generated iOS Info.plist includes Local Network usage description (LAN Sync)', async () => {
    const plistPath = path.join(
        REPO_ROOT,
        'src-tauri/gen/apple/tauritavern_iOS/Info.plist',
    );
    const source = await readFile(plistPath, 'utf8');

    assert.match(source, LOCAL_NETWORK_KEY_PATTERN);
    assert.match(source, /LAN Sync/i);
});

