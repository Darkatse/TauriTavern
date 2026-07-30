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
    const backendSource = await readText('src-tauri/crates/tauritavern/src/presentation/commands/bridge.rs');

    const frontendVersion = frontendSource.match(/SILLYTAVERN_COMPAT_VERSION\s*=\s*['"]([^'"]+)['"]/)?.[1];
    const backendVersion = backendSource.match(/SILLYTAVERN_COMPAT_VERSION:\s*&str\s*=\s*"([^"]+)"/)?.[1];

    assert.equal(frontendVersion, EXPECTED_COMPAT_VERSION);
    assert.equal(backendVersion, EXPECTED_COMPAT_VERSION);
});

test('TauriTavern product version stays aligned across release manifests', async () => {
    const packageJson = JSON.parse(await readText('package.json'));
    const appCargoToml = await readText('src-tauri/crates/tauritavern/Cargo.toml');
    const tauriConfig = JSON.parse(await readText('src-tauri/crates/tauritavern/tauri.conf.json'));

    const appCargoVersion = appCargoToml.match(/^version\s*=\s*"([^"]+)"/m)?.[1];

    assert.ok(appCargoVersion);
    assert.equal(appCargoVersion, packageJson.version);
    assert.equal(tauriConfig.version, packageJson.version);
});
