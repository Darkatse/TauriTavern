import test from 'node:test';
import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import path from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';

const REPO_ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');

async function installHarness(invokeOverride) {
    const calls = [];
    globalThis.window = { __TAURITAVERN__: { api: {} } };
    const { installMcpApi } = await import(pathToFileURL(path.join(
        REPO_ROOT,
        'src/tauri/main/api/mcp.js',
    )));
    installMcpApi({
        safeInvoke: invokeOverride ?? (async (command, args) => {
            calls.push({ command, args });
            return { command, args };
        }),
    });
    return { calls, mcp: globalThis.window.__TAURITAVERN__.api.mcp };
}

test('api.mcp exposes registration, discovery, permission, and one test-call intent', async () => {
    const { calls, mcp } = await installHarness();

    await mcp.servers.list();
    await mcp.servers.create({ displayName: 'Local', endpoint: 'http://127.0.0.1:3000/mcp' });
    await mcp.servers.rename({ registrationId: 'id', displayName: 'Renamed' });
    await mcp.servers.setState({ registrationId: 'id', state: 'active' });
    await mcp.servers.discover('id');
    await mcp.servers.refresh({ registrationId: 'id' });
    await mcp.tools.setPermission({ registrationId: 'id', nativeName: ' search ', permission: 'ask' });
    await mcp.tools.testCall({
        registrationId: 'id',
        nativeName: ' search ',
        argumentsJson: '{"value":9007199254740993}',
    });
    await mcp.servers.remove({ registrationId: 'id' });

    assert.deepEqual(calls.slice(0, 7), [
        { command: 'list_mcp_servers', args: undefined },
        { command: 'create_mcp_server', args: { dto: { displayName: 'Local', endpoint: 'http://127.0.0.1:3000/mcp' } } },
        { command: 'rename_mcp_server', args: { dto: { registrationId: 'id', displayName: 'Renamed' } } },
        { command: 'set_mcp_server_state', args: { dto: { registrationId: 'id', state: 'active' } } },
        { command: 'discover_mcp_tools', args: { dto: { registrationId: 'id' } } },
        { command: 'refresh_mcp_tools', args: { dto: { registrationId: 'id' } } },
        { command: 'set_mcp_tool_permission', args: { dto: { registrationId: 'id', nativeName: ' search ', permission: 'ask' } } },
    ]);
    assert.equal(calls[7].command, 'start_mcp_test_call');
    assert.equal(typeof calls[7].args.dto.callId, 'string');
    assert.equal(calls[8].command, 'test_mcp_tool_call');
    assert.deepEqual(
        { ...calls[8].args.dto, callId: '<generated>' },
        {
            callId: '<generated>',
            registrationId: 'id',
            nativeName: ' search ',
            argumentsJson: '{"value":9007199254740993}',
        },
    );
    assert.equal(calls[7].args.dto.callId, calls[8].args.dto.callId);
    assert.deepEqual(calls[9], {
        command: 'remove_mcp_server',
        args: { dto: { registrationId: 'id' } },
    });
    assert.equal(mcp.callTool, undefined);
    assert.deepEqual(Object.keys(mcp.tools).sort(), ['setPermission', 'testCall']);
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

test('api.mcp AbortSignal requests stop without replacing the backend outcome', async () => {
    const calls = [];
    let resolveCall;
    const callResult = new Promise(resolve => {
        resolveCall = resolve;
    });
    const { mcp } = await installHarness(async (command, args) => {
        calls.push({ command, args });
        return command === 'test_mcp_tool_call' ? callResult : undefined;
    });
    const controller = new AbortController();
    const pending = mcp.tools.testCall({
        registrationId: 'id',
        nativeName: 'search',
        argumentsJson: '{}',
    }, { signal: controller.signal });
    while (calls.length < 2) {
        await Promise.resolve();
    }

    controller.abort();
    await Promise.resolve();

    assert.equal(calls[2].command, 'cancel_mcp_test_call');
    assert.equal(calls[2].args.dto.callId, calls[0].args.dto.callId);
    assert.equal(calls[1].args.dto.callId, calls[0].args.dto.callId);
    resolveCall({
        outcome: 'known_response',
        response: { kind: 'tool_result', isError: false, textBlocks: [], diagnostics: [] },
    });
    assert.equal((await pending).outcome, 'known_response');
});

test('api.mcp proves an already-aborted test call was not sent', async () => {
    const { calls, mcp } = await installHarness();
    const controller = new AbortController();
    controller.abort();

    const outcome = await mcp.tools.testCall({
        registrationId: 'id',
        nativeName: 'search',
        argumentsJson: '{}',
    }, { signal: controller.signal });

    assert.equal(outcome.outcome, 'not_sent');
    assert.equal(calls.length, 0);
});

test('api.mcp cancels after start acknowledgement without dispatching tools/call', async () => {
    const calls = [];
    const blockedCleanup = new Promise(() => {});
    let resolveStart;
    const started = new Promise(resolve => {
        resolveStart = resolve;
    });
    const { mcp } = await installHarness(async (command, args) => {
        calls.push({ command, args });
        if (command === 'start_mcp_test_call') {
            return started;
        }
        if (command === 'cancel_mcp_test_call') {
            return blockedCleanup;
        }
        return undefined;
    });
    const controller = new AbortController();
    const pending = mcp.tools.testCall({
        registrationId: 'id',
        nativeName: 'search',
        argumentsJson: '{}',
    }, { signal: controller.signal });

    controller.abort();
    resolveStart();
    const outcome = await pending;

    assert.equal(outcome.outcome, 'not_sent');
    assert.deepEqual(calls.map(call => call.command), [
        'start_mcp_test_call',
        'cancel_mcp_test_call',
    ]);
    assert.equal(calls[0].args.dto.callId, calls[1].args.dto.callId);
});

test('api.mcp treats a user retry as a new call with new arguments', async () => {
    const { calls, mcp } = await installHarness();

    await mcp.tools.testCall({ registrationId: 'id', nativeName: 'search', argumentsJson: '{"n":1}' });
    await mcp.tools.testCall({ registrationId: 'id', nativeName: 'search', argumentsJson: '{"n":2}' });

    const dispatched = calls.filter(call => call.command === 'test_mcp_tool_call');
    assert.equal(dispatched.length, 2);
    assert.notEqual(dispatched[0].args.dto.callId, dispatched[1].args.dto.callId);
    assert.deepEqual(dispatched.map(call => call.args.dto.argumentsJson), ['{"n":1}', '{"n":2}']);
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
