import test from 'node:test';
import assert from 'node:assert/strict';
import path from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';
import { EventEmitter } from '../src/lib/eventemitter.js';

const REPO_ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
const MODEL_TARGET_EVENT_TYPES = Object.freeze({
    MODEL_TARGET_CREATED: 'test_model_target_created',
    MODEL_TARGET_UPDATED: 'test_model_target_updated',
    MODEL_TARGET_DELETED: 'test_model_target_deleted',
});

async function importConversion() {
    return import(pathToFileURL(path.join(
        REPO_ROOT,
        'src/scripts/extensions/agent-system/src/model-target-conversion.js',
    )));
}

async function importConnection() {
    return import(pathToFileURL(path.join(
        REPO_ROOT,
        'src/scripts/extensions/agent-system/src/model-target-connection.js',
    )));
}

async function importSharedModelTargetConnection() {
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

function installConnectionHarness(targets, options = {}) {
    const savedConnections = [];
    const deletedConnections = [];
    const errors = [];
    const eventSource = new EventEmitter();
    const context = {
        extensionSettings: {
            connectionManager: {
                modelTargets: targets,
            },
        },
        eventSource,
        eventTypes: MODEL_TARGET_EVENT_TYPES,
    };
    globalThis.localStorage = {
        getItem: () => null,
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
                    delete: async ({ connectionId }) => {
                        deletedConnections.push(connectionId);
                        const error = options.deleteErrors?.[connectionId];
                        if (error) {
                            throw error;
                        }
                    },
                },
            },
        },
        toastr: {
            error: (message) => errors.push(message),
        },
    };

    return {
        savedConnections,
        deletedConnections,
        errors,
        eventSource,
        eventTypes: MODEL_TARGET_EVENT_TYPES,
    };
}

async function captureConsole(operation) {
    const original = {
        debug: console.debug,
        warn: console.warn,
        error: console.error,
    };
    const calls = [];
    console.debug = (...args) => calls.push({ level: 'debug', args });
    console.warn = (...args) => calls.push({ level: 'warn', args });
    console.error = (...args) => calls.push({ level: 'error', args });
    try {
        return await operation(calls);
    } finally {
        console.debug = original.debug;
        console.warn = original.warn;
        console.error = original.error;
    }
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
    } = await importSharedModelTargetConnection();
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
    } = await importSharedModelTargetConnection();

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

test('Agent model target connection sync follows saved Model Target updates', async () => {
    const target = sampleTarget();
    const updatedTarget = sampleTarget({
        secretRef: {
            key: 'api_key_custom',
            id: 'secret-rotated',
            labelSnapshot: 'Rotated custom key',
        },
    });
    const {
        savedConnections,
        deletedConnections,
        errors,
        eventSource,
        eventTypes,
    } = installConnectionHarness([target]);
    const {
        startModelTargetLlmConnectionSync,
        subscribeModelTargetChanges,
        syncSavedModelTargetLlmConnections,
    } = await importConnection();

    const result = await syncSavedModelTargetLlmConnections();
    assert.equal(result.synced, 1);
    assert.deepEqual(result.failed, []);
    assert.equal(savedConnections.at(-1).auth.secretRef.id, 'secret-custom');

    const stopSync = startModelTargetLlmConnectionSync();
    const observedChanges = [];
    const unsubscribeChanges = subscribeModelTargetChanges((change) => observedChanges.push(change));
    try {
        await captureConsole(() => eventSource.emit(eventTypes.MODEL_TARGET_UPDATED, target, updatedTarget));
        assert.equal(savedConnections.at(-1).auth.secretRef.id, 'secret-rotated');
        assert.equal(observedChanges.at(-1).type, 'updated');

        const savedCountAfterUpdate = savedConnections.length;
        await captureConsole(() => eventSource.emit(eventTypes.MODEL_TARGET_DELETED, updatedTarget));
        assert.equal(savedConnections.length, savedCountAfterUpdate);
        assert.equal(observedChanges.at(-1).type, 'deleted');
        assert.deepEqual(deletedConnections, []);
        assert.deepEqual(errors, []);
    } finally {
        unsubscribeChanges();
        stopSync();
    }
});

test('Agent model target startup sync invalidates stale LLM connection when materialization fails', async () => {
    const invalidTarget = sampleTarget({ proxy: 'corporate-proxy' });
    const { savedConnections, deletedConnections } = installConnectionHarness([invalidTarget]);
    const {
        syncSavedModelTargetLlmConnections,
    } = await importConnection();

    const result = await captureConsole(() => syncSavedModelTargetLlmConnections());

    assert.equal(result.synced, 0);
    assert.equal(result.failed.length, 1);
    assert.equal(result.failed[0].invalidation.connectionId, 'model-target-writer-target');
    assert.equal(result.failed[0].invalidation.deleted, true);
    assert.equal(savedConnections.length, 0);
    assert.deepEqual(deletedConnections, ['model-target-writer-target']);
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
