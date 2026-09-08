import test from 'node:test';
import assert from 'node:assert/strict';
import path from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';

const REPO_ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');

async function importConversion() {
    return import(pathToFileURL(path.join(
        REPO_ROOT,
        'src/scripts/tauritavern/agent/model-target-llm-connection.js',
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

function installConnectionHarness(targets) {
    const savedConnections = [];
    const context = {
        extensionSettings: {
            connectionManager: {
                modelTargets: targets,
            },
        },
    };
    globalThis.window = {
        SillyTavern: {
            getContext: () => context,
        },
        __TAURITAVERN__: {
            api: {
                llmConnections: {
                    save: async ({ connection }) => {
                        savedConnections.push(structuredClone(connection));
                    },
                },
            },
        },
    };

    return { savedConnections };
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


test('Agent model target conversion preserves native adapter opt-ins', async () => {
    const { buildLlmConnectionFromModelTarget } = await importConversion();
    const connection = buildLlmConnectionFromModelTarget(sampleTarget({
        api: 'custom_openai_responses',
        model: 'deepseek-chat',
        'custom-api-format': 'openai_responses',
        adapterHints: {
            openaiResponsesMode: 'websocket',
        },
    }));

    assert.deepEqual(connection.adapterHints, {
        openaiResponsesMode: 'websocket',
    });
    assert.deepEqual(connection.capabilities, {});
});

test('Gemini model targets preserve the Custom source and explicit native format', async () => {
    const { buildLlmConnectionFromModelTarget } = await importConversion();
    for (const api of ['custom', 'custom_gemini_generate_content']) {
        const connection = buildLlmConnectionFromModelTarget(sampleTarget({
            api, model: 'gemini-test', 'custom-api-format': 'gemini_generate_content',
            'api-url': 'https://example.test/v1beta',
        }));
        assert.deepEqual(connection.provider, {
            chatCompletionSource: 'custom', customApiFormat: 'gemini_generate_content',
        });
        assert.equal(connection.endpoint.baseUrl, 'https://example.test/v1beta');
        assert.equal(connection.auth.secretRef.key, 'api_key_custom');
    }
});

test('Agent model target conversion keeps OpenCode service and wire format', async () => {
    const { buildLlmConnectionFromModelTarget } = await importConversion();
    const connection = buildLlmConnectionFromModelTarget(sampleTarget({
        api: 'opencode',
        model: 'qwen3-coder',
        'api-url': 'go',
        'custom-api-format': 'claude_messages',
        secretRef: { key: 'api_key_opencode', id: 'secret-opencode' },
    }));

    assert.deepEqual(connection.provider, { chatCompletionSource: 'opencode' });
    assert.deepEqual(connection.endpoint.sourceSpecific, {
        opencode_endpoint: 'go',
        opencode_api_format: 'claude_messages',
    });
});



test('Agent run model target ensure refreshes by connection ref without adopting target model changes', async () => {
    const currentTarget = sampleTarget({
        model: 'claude-4-sonnet',
        secretRef: {
            key: 'api_key_custom',
            id: 'secret-current',
        },
    });
    const { savedConnections } = installConnectionHarness([currentTarget]);
    const {
        ensureModelTargetLlmConnectionForProfile,
    } = await importConversion();
    const profile = {
        model: {
            mode: 'connectionRef',
            connectionRef: 'model-target-writer-target',
            modelId: 'claude-3-7-sonnet',
        },
    };

    await ensureModelTargetLlmConnectionForProfile(profile);

    assert.equal(profile.model.modelId, 'claude-3-7-sonnet');
    assert.equal(savedConnections.length, 1);
    assert.equal(savedConnections[0].auth.secretRef.id, 'secret-current');
});


test('Agent run model target ensure fails fast when the saved target binding is missing', async () => {
    installConnectionHarness([]);
    const {
        ensureModelTargetLlmConnectionForProfile,
    } = await importConversion();

    await assert.rejects(
        () => ensureModelTargetLlmConnectionForProfile({
            model: {
                mode: 'connectionRef',
                connectionRef: 'model-target-writer-target',
                modelId: 'claude-3-7-sonnet',
            },
        }),
        /agent\.model_target_binding_missing/,
    );
});

test('Agent model target conversion rejects lossy or invalid targets', async () => {
    const {
        buildLlmConnectionFromModelTarget,
        modelTargetConnectionRef,
    } = await importConversion();

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

test('Agent model targets preserve a named reverse proxy and the provider-specific model name', async () => {
    const { buildLlmConnectionFromModelTarget, modelBindingFromTarget } = await importConversion();
    const target = sampleTarget({
        api: 'google', model: '[v]gemini-custom', proxy: 'Team proxy',
        secretRef: { key: 'api_key_makersuite', id: 'google-key' },
    });
    const connection = buildLlmConnectionFromModelTarget(target);
    assert.deepEqual(connection.routing, { reverseProxy: { preset: 'Team proxy' } });
    assert.equal(connection.provider.chatCompletionSource, 'makersuite');
    assert.equal(modelBindingFromTarget(target).modelId, '[v]gemini-custom');
    assert.deepEqual(buildLlmConnectionFromModelTarget({ ...target, secretRef: undefined }).auth, {});
    assert.deepEqual(buildLlmConnectionFromModelTarget({ ...target, proxy: 'None' }).routing, {});
});
