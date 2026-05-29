import test from 'node:test';
import assert from 'node:assert/strict';
import path from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';

const REPO_ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');

async function importConversion() {
    return import(pathToFileURL(path.join(
        REPO_ROOT,
        'src/scripts/extensions/agent-system/src/model-target-conversion.js',
    )));
}

function sampleTarget(overrides = {}) {
    return {
        schemaVersion: 1,
        kind: 'tauritavern.modelTarget',
        id: 'Writer Target',
        mode: 'cc',
        name: 'Writer model',
        api: 'custom_claude_messages',
        model: 'claude-3-7-sonnet',
        'api-url': 'https://example.test/v1',
        secretRef: {
            key: 'api_key_custom',
            id: 'secret-custom',
            labelSnapshot: 'Custom key',
        },
        ...overrides,
    };
}

test('Agent model target conversion materializes LLM connection and profile binding', async () => {
    const {
        buildLlmConnectionFromModelTarget,
        findModelTargetForBinding,
        modelBindingFromTarget,
        modelTargetConnectionRef,
    } = await importConversion();
    const target = sampleTarget();

    assert.equal(modelTargetConnectionRef(target), 'model-target-writer-target');
    assert.deepEqual(modelBindingFromTarget(target), {
        mode: 'connectionRef',
        connectionRef: 'model-target-writer-target',
        modelId: 'claude-3-7-sonnet',
    });
    assert.deepEqual(buildLlmConnectionFromModelTarget(target), {
        schemaVersion: 1,
        kind: 'tauritavern.llmConnection',
        id: 'model-target-writer-target',
        displayName: 'Writer model',
        description: 'Connection Manager model target: Writer model',
        provider: {
            chatCompletionSource: 'custom',
            customApiFormat: 'claude_messages',
        },
        endpoint: {
            baseUrl: 'https://example.test/v1',
            sourceSpecific: {},
        },
        auth: {
            secretRef: {
                key: 'api_key_custom',
                id: 'secret-custom',
                labelSnapshot: 'Custom key',
            },
        },
        routing: {},
        adapterHints: {},
        capabilities: {},
    });
    assert.equal(findModelTargetForBinding([target], {
        mode: 'connectionRef',
        connectionRef: 'model-target-writer-target',
        modelId: 'claude-3-7-sonnet',
    }), target);
});

test('Agent model target conversion rejects lossy or invalid targets', async () => {
    const {
        buildLlmConnectionFromModelTarget,
        modelTargetConnectionRef,
    } = await importConversion();

    assert.throws(
        () => buildLlmConnectionFromModelTarget(sampleTarget({ proxy: 'corporate-proxy' })),
        /cannot be converted to an Agent LLM connection/,
    );
    assert.throws(
        () => buildLlmConnectionFromModelTarget(sampleTarget({ mode: 'tc' })),
        /is not a chat-completion target/,
    );
    assert.throws(
        () => buildLlmConnectionFromModelTarget(sampleTarget({ secretRef: null })),
        /missing secret reference/,
    );
    assert.throws(
        () => modelTargetConnectionRef({ id: 'x'.repeat(129) }),
        /too long/,
    );
});
