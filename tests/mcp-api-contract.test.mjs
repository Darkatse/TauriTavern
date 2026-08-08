import test from 'node:test';
import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import path from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';

const REPO_ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');

async function installHarness() {
    const calls = [];
    globalThis.window = { __TAURITAVERN__: { api: {} } };
    const { installMcpApi } = await import(pathToFileURL(path.join(
        REPO_ROOT,
        'src/tauri/main/api/mcp.js',
    )));
    installMcpApi({
        safeInvoke: async (command, args) => {
            calls.push({ command, args });
            return { command, args };
        },
    });
    return { calls, mcp: globalThis.window.__TAURITAVERN__.api.mcp };
}

test('api.mcp exposes only registration, discovery, and permission intents', async () => {
    const { calls, mcp } = await installHarness();

    await mcp.servers.list();
    await mcp.servers.create({ displayName: 'Local', endpoint: 'http://127.0.0.1:3000/mcp' });
    await mcp.servers.rename({ registrationId: 'id', displayName: 'Renamed' });
    await mcp.servers.setState({ registrationId: 'id', state: 'active' });
    await mcp.servers.discover('id');
    await mcp.tools.setPermission({ registrationId: 'id', nativeName: ' search ', permission: 'ask' });
    await mcp.servers.remove({ registrationId: 'id' });

    assert.deepEqual(calls, [
        { command: 'list_mcp_servers', args: undefined },
        { command: 'create_mcp_server', args: { dto: { displayName: 'Local', endpoint: 'http://127.0.0.1:3000/mcp' } } },
        { command: 'rename_mcp_server', args: { dto: { registrationId: 'id', displayName: 'Renamed' } } },
        { command: 'set_mcp_server_state', args: { dto: { registrationId: 'id', state: 'active' } } },
        { command: 'discover_mcp_tools', args: { dto: { registrationId: 'id' } } },
        { command: 'set_mcp_tool_permission', args: { dto: { registrationId: 'id', nativeName: ' search ', permission: 'ask' } } },
        { command: 'remove_mcp_server', args: { dto: { registrationId: 'id' } } },
    ]);
    assert.equal(mcp.callTool, undefined);
});

test('api.mcp fails fast on invalid states and permissions', async () => {
    const { mcp } = await installHarness();

    await assert.rejects(
        () => mcp.servers.setState({ registrationId: 'id', state: 'connected' }),
        /state must be active or paused/,
    );
    await assert.rejects(
        () => mcp.tools.setPermission({ registrationId: 'id', nativeName: 'search', permission: 'always' }),
        /permission must be off, ask, or allow/,
    );
    await assert.rejects(
        () => mcp.tools.setPermission({ registrationId: 'id', nativeName: '', permission: 'ask' }),
        /nativeName is required/,
    );
});

test('MCP Manager is owned by a first-party extension, not TauriTavern Settings', async () => {
    const [manifestRaw, settingsApp, settingsPopup] = await Promise.all([
        readFile(path.join(REPO_ROOT, 'src/scripts/extensions/mcp-manager/manifest.json'), 'utf8'),
        readFile(path.join(REPO_ROOT, 'src/scripts/tauri/setting/settings-app/SettingsApp.js'), 'utf8'),
        readFile(path.join(REPO_ROOT, 'src/scripts/tauri/setting/setting-panel/settings-popup.js'), 'utf8'),
    ]);
    const manifest = JSON.parse(manifestRaw);

    assert.equal(manifest.js, 'dist/index.bundle.js');
    assert.equal(manifest.css, 'style.css');
    assert.doesNotMatch(settingsApp, /MCP Servers|openMcpManager/);
    assert.doesNotMatch(settingsPopup, /openMcpManager|mcp-manager/);
});
