import test from 'node:test';
import assert from 'node:assert/strict';
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

test('api.mcp sends the test call that was requested even if the caller reuses its input', async () => {
    const calls = [];
    let resolveStart;
    const started = new Promise(resolve => {
        resolveStart = resolve;
    });
    const { mcp } = await installHarness(async (command, args) => {
        calls.push({ command, args });
        return command === 'start_mcp_test_call' ? started : undefined;
    });
    const input = { registrationId: 'server-a', nativeName: 'read', argumentsJson: '{}' };

    const pending = mcp.tools.testCall(input);
    Object.assign(input, { registrationId: 'server-b', nativeName: 'write' });
    resolveStart();
    await pending;

    assert.equal(calls[1].command, 'test_mcp_tool_call');
    assert.equal(calls[1].args.dto.registrationId, 'server-a');
    assert.equal(calls[1].args.dto.nativeName, 'read');
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
