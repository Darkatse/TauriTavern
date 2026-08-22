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
    ensureCustomEvent();
    globalThis.window = new EventTarget();
    globalThis.window.__TAURITAVERN__ = { api: {} };

    const { installLlmConnectionsApi } = await import(pathToFileURL(path.join(
        REPO_ROOT,
        'src/tauri/main/api/llm-connection.js',
    )));
    installLlmConnectionsApi({
        safeInvoke: async (command, args) => {
            return { command, args };
        },
    });

    return {
        llmConnections: globalThis.window.__TAURITAVERN__.api.llmConnections,
    };
}

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
