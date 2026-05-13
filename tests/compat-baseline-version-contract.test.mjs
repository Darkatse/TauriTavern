import test from 'node:test';
import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const REPO_ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
const EXPECTED_COMPAT_VERSION = '1.18.0';

async function readText(relativePath) {
    return readFile(path.join(REPO_ROOT, relativePath), 'utf8');
}

test('SillyTavern compatibility baseline stays aligned across frontend and backend', async () => {
    const frontendSource = await readText('src/compat-version.js');
    const backendSource = await readText('src-tauri/src/presentation/commands/bridge.rs');

    const frontendVersion = frontendSource.match(/SILLYTAVERN_COMPAT_VERSION\s*=\s*['"]([^'"]+)['"]/)?.[1];
    const backendVersion = backendSource.match(/SILLYTAVERN_COMPAT_VERSION:\s*&str\s*=\s*"([^"]+)"/)?.[1];

    assert.equal(frontendVersion, EXPECTED_COMPAT_VERSION);
    assert.equal(backendVersion, EXPECTED_COMPAT_VERSION);
});
