import test from 'node:test';
import assert from 'node:assert/strict';
import path from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';

const REPO_ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');

function ensureCustomEvent() {
    if (typeof globalThis.CustomEvent === 'function') {
        return;
    }

    globalThis.CustomEvent = class CustomEvent extends Event {
        constructor(type, options = {}) {
            super(type, options);
            this.detail = options.detail;
        }
    };
}

async function installHarness() {
    const calls = [];
    ensureCustomEvent();
    globalThis.window = new EventTarget();
    globalThis.window.__TAURITAVERN__ = { api: {} };

    const { installLlmConnectionsApi } = await import(pathToFileURL(path.join(
        REPO_ROOT,
        'src/tauri/main/api/llm-connection.js',
    )));
    installLlmConnectionsApi({
        safeInvoke: async (command, args) => {
            calls.push({ command, args });
            return { command, args };
        },
    });

    return {
        calls,
        llmConnections: globalThis.window.__TAURITAVERN__.api.llmConnections,
    };
}

test('api.llmConnections forwards camelCase DTOs', async () => {
    const { calls, llmConnections } = await installHarness();
    const connection = {
        schemaVersion: 1,
        kind: 'tauritavern.llmConnection',
        id: 'model-target-main',
        displayName: 'Main model',
        provider: { chatCompletionSource: 'openai' },
        auth: { secretRef: { key: 'api_key_openai', id: 'secret-openai' } },
    };

    await llmConnections.list();
    await llmConnections.load('model-target-main');
    await llmConnections.save({ connection });
    await llmConnections.delete({ connectionId: 'model-target-main' });

    assert.deepEqual(calls, [
        { command: 'list_llm_connections', args: undefined },
        { command: 'load_llm_connection', args: { dto: { connectionId: 'model-target-main' } } },
        { command: 'save_llm_connection', args: { dto: { connection } } },
        { command: 'delete_llm_connection', args: { dto: { connectionId: 'model-target-main' } } },
    ]);
});

test('api.llmConnections publishes connection change events after successful mutations', async () => {
    const { llmConnections } = await installHarness();
    const { subscribeLlmConnectionsChanged } = await import(pathToFileURL(path.join(
        REPO_ROOT,
        'src/scripts/tauritavern/agent/llm-connection-events.js',
    )));
    const events = [];
    const unsubscribe = subscribeLlmConnectionsChanged(() => {
        events.push('changed');
    });

    await llmConnections.save({
        connection: {
            schemaVersion: 1,
            kind: 'tauritavern.llmConnection',
            id: 'model-target-main',
            displayName: 'Main model',
            provider: { chatCompletionSource: 'openai' },
            auth: { secretRef: { key: 'api_key_openai', id: 'secret-openai' } },
        },
    });
    await llmConnections.delete('model-target-main');
    unsubscribe();

    assert.deepEqual(events, ['changed', 'changed']);
});

test('api.llmConnections fails fast on invalid inputs', async () => {
    const { llmConnections } = await installHarness();

    await assert.rejects(
        () => llmConnections.load({ connectionId: '' }),
        /connectionId is required/,
    );
    await assert.rejects(
        () => llmConnections.delete(''),
        /connectionId is required/,
    );
    await assert.rejects(
        () => llmConnections.save(null),
        /connection must be an object/,
    );
});
