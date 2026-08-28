import test from 'node:test';
import assert from 'node:assert/strict';
import path from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';

const REPO_ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');

async function importFresh(relativePath) {
    const modulePath = path.join(REPO_ROOT, relativePath);
    const url = `${pathToFileURL(modulePath).href}?t=${Date.now()}-${Math.random()}`;
    return import(url);
}

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

function installWindow(api) {
    ensureCustomEvent();
    const window = new EventTarget();
    window.__TAURITAVERN__ = { api };
    globalThis.window = window;
    return window;
}

function cloneJson(value) {
    return JSON.parse(JSON.stringify(value));
}

function installRollbackEventCapture(script, updates = []) {
    script.event_types = {
        ...(script.event_types || {}),
        MESSAGE_UPDATED: 'message_updated',
    };
    script.eventSource = {
        async emit(event, messageId) {
            updates.push({ event, messageId });
        },
    };
    return script;
}

test('Agent System settings use the extension store and publish changes', async () => {
    const writes = [];
    let stored = null;
    installWindow({
        extension: {
            store: {
                async tryGetJson() {
                    if (stored === null) {
                        return { found: false };
                    }
                    return { found: true, value: stored };
                },
                async setJson(request) {
                    writes.push(request);
                    stored = request.value;
                },
            },
        },
    });

    const settings = await importFresh('src/scripts/tauritavern/agent/agent-system-settings.js');
    const loaded = await settings.loadAgentSystemSettings();
    assert.deepEqual(loaded, {
        agentModeEnabled: false,
        chatInputToggleHidden: false,
        activeProfileId: 'default-writer',
        editingProfileId: 'default-writer',
        activeTab: 'profiles',
        runTimelineHeightPx: null,
    });
    assert.equal(writes.length, 0);

    stored = {
        agentModeEnabled: true,
        selectedProfileId: 'legacy-writer',
    };
    assert.deepEqual(await settings.loadAgentSystemSettings(), {
        agentModeEnabled: true,
        chatInputToggleHidden: false,
        activeProfileId: 'legacy-writer',
        editingProfileId: 'legacy-writer',
        activeTab: 'profiles',
        runTimelineHeightPx: null,
    });

    let emitted = null;
    const unsubscribe = settings.subscribeAgentSystemSettings((next) => {
        emitted = next;
    });
    const saved = await settings.saveAgentSystemSettings({
        agentModeEnabled: true,
        chatInputToggleHidden: true,
        activeProfileId: 'writer',
        editingProfileId: 'editor',
    });
    unsubscribe();

    assert.deepEqual(saved, {
        agentModeEnabled: true,
        chatInputToggleHidden: true,
        activeProfileId: 'writer',
        editingProfileId: 'editor',
        activeTab: 'profiles',
        runTimelineHeightPx: null,
    });
    assert.deepEqual(emitted, saved);
});






test('Agent generation router refreshes Model Target LLM connection before Agent Mode options', async () => {
    const currentTarget = {
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
            id: 'secret-current',
            labelSnapshot: 'Current custom key',
        },
    };
    const savedConnections = [];
    const window = installWindow({
        extension: {
            store: {
                async tryGetJson() {
                    return {
                        found: true,
                        value: {
                            agentModeEnabled: true,
                            activeProfileId: 'writer',
                            editingProfileId: 'writer',
                            activeTab: 'profiles',
                            runTimelineHeightPx: null,
                        },
                    };
                },
            },
        },
        llmConnections: {
            async save({ connection }) {
                savedConnections.push(connection);
            },
        },
        agent: {
            profiles: {
                async load({ profileId }) {
                    assert.equal(profileId, 'writer');
                    return {
                        profile: {
                            run: { directRunnable: true },
                            model: {
                                mode: 'connectionRef',
                                connectionRef: 'model-target-writer-target',
                                modelId: 'claude-3-7-sonnet',
                            },
                            context: {
                                initialChatHistoryMessages: 4,
                                includeActivatedWorldInfo: false,
                            },
                        },
                    };
                },
                async resolveSystemPrompt({ profileId }) {
                    assert.equal(profileId, 'writer');
                    assert.equal(savedConnections.length, 1);
                    return { agentSystemPrompt: 'Resolved Agent System Prompt.' };
                },
            },
        },
    });
    window.SillyTavern = {
        getContext: () => ({
            extensionSettings: {
                connectionManager: {
                    modelTargets: [currentTarget],
                },
            },
        }),
    };

    const router = await importFresh('src/scripts/tauritavern/agent/agent-generation-router.js');

    const options = await router.getAgentGenerationOptions({
        generationType: 'normal',
        mainApi: 'openai',
    });

    assert.equal(savedConnections.length, 1);
    assert.equal(savedConnections[0].auth.secretRef.id, 'secret-current');
    assert.deepEqual(options.agentContextPolicy, {
        initialChatHistoryMessages: 4,
        includeActivatedWorldInfo: false,
    });
});

test('Agent generation router rejects non-direct callable profiles before direct generation', async () => {
    installWindow({
        extension: {
            store: {
                async tryGetJson() {
                    return {
                        found: true,
                        value: {
                            agentModeEnabled: true,
                            activeProfileId: 'subagent-only',
                            editingProfileId: 'subagent-only',
                            activeTab: 'profiles',
                            runTimelineHeightPx: null,
                        },
                    };
                },
            },
        },
        agent: {
            profiles: {
                async load({ profileId }) {
                    assert.equal(profileId, 'subagent-only');
                    return {
                        profile: {
                            run: { directRunnable: false },
                            context: {
                                initialChatHistoryMessages: -1,
                                includeActivatedWorldInfo: true,
                            },
                        },
                    };
                },
                async resolveSystemPrompt() {
                    throw new Error('resolveSystemPrompt should not run for non-direct callable direct generation');
                },
            },
        },
    });

    const router = await importFresh('src/scripts/tauritavern/agent/agent-generation-router.js');

    await assert.rejects(
        () => router.getAgentGenerationOptions({ generationType: 'normal', mainApi: 'openai' }),
        /agent\.profile_not_direct_runnable/,
    );
});


test('Agent context policy windows latest-first prompt history without mutating frozen input', async () => {
    const contextPolicy = await importFresh('src/scripts/tauritavern/agent/agent-context-policy.js');
    const chat = [
        { role: 'user', content: 'latest' },
        { role: 'assistant', content: 'middle' },
        { role: 'user', content: 'oldest' },
    ];

    assert.deepEqual(contextPolicy.normalizeAgentContextPolicy({
        initialChatHistoryMessages: 0,
        includeActivatedWorldInfo: true,
    }), {
        initialChatHistoryMessages: 0,
        includeActivatedWorldInfo: true,
    });
    assert.deepEqual(contextPolicy.applyInitialChatHistoryPolicy(chat, {
        initialChatHistoryMessages: 0,
        includeActivatedWorldInfo: true,
    }), []);
    assert.deepEqual(contextPolicy.applyInitialChatHistoryPolicy(chat, {
        initialChatHistoryMessages: 2,
        includeActivatedWorldInfo: true,
    }), chat.slice(0, 2));
    assert.equal(contextPolicy.applyInitialChatHistoryPolicy(chat, {
        initialChatHistoryMessages: -1,
        includeActivatedWorldInfo: true,
    }), chat);

    const materialized = contextPolicy.materializeInitialChatHistoryMessages(chat, {
        initialChatHistoryMessages: -1,
        includeActivatedWorldInfo: true,
    });
    assert.deepEqual(materialized, chat);
    assert.notEqual(materialized, chat);
    assert.notEqual(materialized[0], chat[0]);

    materialized[0].content = 'mutated';
    assert.equal(chat[0].content, 'latest');

    assert.throws(
        () => contextPolicy.applyInitialChatHistoryPolicy(null, {
            initialChatHistoryMessages: -1,
            includeActivatedWorldInfo: true,
        }),
        /agent\.context_history_messages_invalid/,
    );
});










test('Agent run controller tracks active runs until terminal events', async () => {
    let listener = null;
    let stopped = false;
    installWindow({
        agent: {
            async startRunWithPromptSnapshot(input) {
                return { runId: 'run-1', input };
            },
            subscribe(runId, callback) {
                assert.equal(runId, 'run-1');
                listener = callback;
                return () => {
                    stopped = true;
                };
            },
            async settleChatPresentation() {},
        },
    });

    const controller = await importFresh('src/scripts/tauritavern/agent/agent-run-controller.js');
    const stateChanges = [];
    const unsubscribe = controller.subscribeAgentRunState((state) => {
        stateChanges.push(state);
    });

    const run = controller.startAndWaitForAgentRun({ generationType: 'normal' });
    await Promise.resolve();

    assert.equal(controller.hasActiveAgentRun(), true);
    assert.equal(controller.getActiveAgentRun().runId, 'run-1');

    listener({ type: 'run_step_started', payload: {} });
    listener({ type: 'run_completed', payload: { messageId: 'mes-1' } });
    const result = await run;
    unsubscribe();

    assert.equal(result.handle.runId, 'run-1');
    assert.equal(result.terminalEvent.type, 'run_completed');
    assert.equal(stopped, true);
    assert.equal(controller.hasActiveAgentRun(), false);
    assert.equal(stateChanges.at(-1).lastEvent.type, 'run_completed');
});

test('Agent run controller waits for retained chat output to settle', async () => {
    let listener = null;
    let releasePresentation;
    const presentation = new Promise(resolve => { releasePresentation = resolve; });
    installWindow({
        agent: {
            async startRunWithPromptSnapshot() {
                return { runId: 'run-presentation' };
            },
            subscribe(_runId, callback) {
                listener = callback;
                return () => {};
            },
            settleChatPresentation(handle) {
                assert.equal(handle.runId, 'run-presentation');
                return presentation;
            },
        },
    });

    const controller = await importFresh('src/scripts/tauritavern/agent/agent-run-controller.js');
    let settled = false;
    const run = controller.startAndWaitForAgentRun({ generationType: 'normal' })
        .finally(() => { settled = true; });
    await Promise.resolve();
    listener({ type: 'run_completed', payload: {} });
    await Promise.resolve();
    assert.equal(settled, false);
    assert.equal(controller.hasActiveAgentRun(), true);

    releasePresentation();
    await run;
    assert.equal(settled, true);
    assert.equal(controller.hasActiveAgentRun(), false);
});




test('Agent run controller clears active state when subscription setup fails', async () => {
    installWindow({
        agent: {
            async startRunWithPromptSnapshot() {
                return { runId: 'run-2' };
            },
            subscribe() {
                throw new Error('subscribe failed');
            },
        },
    });

    const controller = await importFresh('src/scripts/tauritavern/agent/agent-run-controller.js');

    await assert.rejects(
        () => controller.startAndWaitForAgentRun({ generationType: 'normal' }),
        /subscribe failed/,
    );
    assert.equal(controller.hasActiveAgentRun(), false);
});





test('Agent error presenter surfaces userRetryable from run_failed payload', async () => {
    const presenter = await importFresh('src/scripts/tauritavern/agent/agent-error-presenter.js');

    const drift = presenter.presentAgentRunFailure({
        payload: {
            code: 'model.tool_call_required',
            message: 'low-level message',
            technicalMessage: 'Validation error: model.tool_call_required',
            retryable: false,
            userRetryable: true,
        },
    });
    assert.equal(drift.code, 'model.tool_call_required');
    assert.equal(drift.retryable, false);
    assert.equal(drift.userRetryable, true);
    assert.match(drift.message, /Agent tool flow/);

    const transient = presenter.presentAgentRunFailure({
        payload: { code: 'transient', message: 'busy', retryable: true },
    });
    assert.equal(transient.retryable, true);
    assert.equal(transient.userRetryable, true);

    const fatal = presenter.presentAgentRunFailure({
        payload: { code: 'agent.internal_error', message: 'boom', retryable: false },
    });
    assert.equal(fatal.userRetryable, false);

    const unconfigured = presenter.presentAgentRunFailure({
        payload: {
            code: 'agent.profile_model_requires_configuration',
            message: 'Agent profile `imported-writer` requires a local model selection before it can run',
            retryable: false,
        },
    });
    assert.match(unconfigured.message, /local model selection/);
    assert.equal(unconfigured.userRetryable, false);

    assert.match(
        presenter.agentErrorMessage(new Error('agent.profile_model_requires_configuration: imported-writer needs a model')),
        /local model selection/,
    );
});



test('Agent retry resolves typed generation intent instead of clicking regenerate UI', async () => {
    const retry = await importFresh('src/scripts/tauritavern/agent/agent-run-retry.js');

    assert.equal(retry.retryGenerationTypeFor('normal'), 'regenerate');
    assert.equal(retry.retryGenerationTypeFor('regenerate'), 'regenerate');
    assert.equal(retry.retryGenerationTypeFor('swipe'), 'swipe');
    assert.throws(
        () => retry.retryGenerationTypeFor('continue'),
        /agent\.retry_generation_type_unsupported/,
    );
    assert.equal(retry.resolveAgentRunGenerationType({
        events: [
            { type: 'run_created', payload: {} },
            { type: 'generation_intent_recorded', payload: { generationType: 'swipe' } },
        ],
    }), 'swipe');

    const calls = [];
    const result = await retry.retryAgentRunFailure({
        run: { generationType: 'swipe' },
        terminalEvent: { type: 'run_failed', payload: { userRetryable: true } },
        runtime: {
            mainApi: 'openai',
            selectedGroup: null,
            async getAgentGenerationOptions(input) {
                calls.push({ kind: 'options', input });
                return { agentMode: true, agentProfileId: 'writer' };
            },
            async Generate(type, options) {
                calls.push({ kind: 'generate', type, options });
                return 'retried';
            },
        },
    });

    assert.equal(result, 'retried');
    assert.deepEqual(calls, [
        {
            kind: 'options',
            input: { generationType: 'swipe', mainApi: 'openai', selectedGroup: null },
        },
        {
            kind: 'generate',
            type: 'swipe',
            options: { agentMode: true, agentProfileId: 'writer' },
        },
    ]);

    await assert.rejects(
        () => retry.retryAgentRunFailure({
            terminalEvent: { type: 'run_failed', payload: { userRetryable: true } },
            runtime: {
                mainApi: 'openai',
                selectedGroup: null,
                async getAgentGenerationOptions() {
                    return { agentMode: true };
                },
                async Generate() {},
            },
        }),
        /agent\.retry_generation_intent_missing/,
    );
    await assert.rejects(
        () => retry.retryAgentRunFailure({
            run: { generationType: 'normal' },
            terminalEvent: { type: 'run_failed', payload: { userRetryable: true } },
            runtime: {
                mainApi: 'openai',
                selectedGroup: null,
                async getAgentGenerationOptions() {
                    return {};
                },
                async Generate() {},
            },
        }),
        /agent\.retry_agent_mode_disabled/,
    );
});






test('Rollback helper deletes drift messages back-to-front and dedupes targets', async () => {
    const { rollbackAgentRunDriftMessages } = await importFresh('src/scripts/tauritavern/agent/agent-run-message-rollback.js');

    const chat = [
        { extra: { tauritavern: { agent: { runId: 'run-x', rollback: { strategy: 'deleteMessage' } } } } },
        { extra: { tauritavern: { agent: { runId: 'other-run' } } } },
        { extra: { tauritavern: { agent: { runId: 'run-x', rollback: { strategy: 'deleteMessage' } } } } },
    ];
    const deletions = [];
    const updates = [];
    const script = {
        chat,
        async deleteMessage(index) {
            deletions.push(index);
            chat.splice(index, 1);
        },
    };
    installRollbackEventCapture(script, updates);

    const result = await rollbackAgentRunDriftMessages({
        runId: 'run-x',
        targets: [
            { messageId: '0' },
            { messageId: '2' },
            { messageId: '2' },
        ],
        script,
    });

    assert.deepEqual(deletions, [2, 0]);
    assert.equal(result.attempted, 2);
    assert.equal(result.deleted, 2);
    assert.equal(result.swipesRemoved, 0);
    assert.equal(chat.length, 1);
    assert.equal(chat[0].extra.tauritavern.agent.runId, 'other-run');
    assert.deepEqual(updates, [{ event: 'message_updated', messageId: 0 }]);
});



test('Rollback helper fails fast instead of deleting a message when swipe metadata is unsafe', async () => {
    const { rollbackAgentRunDriftMessages } = await importFresh('src/scripts/tauritavern/agent/agent-run-message-rollback.js');

    const chat = [
        {
            is_user: false,
            swipes: ['only one'],
            swipe_id: 0,
            extra: {
                tauritavern: {
                    agent: {
                        runId: 'run-edge',
                        rollback: { strategy: 'deleteSwipe', swipeId: 0 },
                    },
                },
            },
        },
    ];
    const swipeCalls = [];
    const messageDeletions = [];
    const updates = [];
    const script = {
        chat,
        async deleteSwipe(swipeId, messageId) {
            swipeCalls.push({ swipeId, messageId });
        },
        async deleteMessage(index) {
            messageDeletions.push(index);
            chat.splice(index, 1);
        },
    };
    installRollbackEventCapture(script, updates);

    await assert.rejects(
        () => rollbackAgentRunDriftMessages({
            runId: 'run-edge',
            targets: [{ messageId: '0' }],
            script,
        }),
        /agent\.rollback_swipe_state_invalid/,
    );

    assert.deepEqual(swipeCalls, [], 'must not call deleteSwipe when only one swipe remains');
    assert.deepEqual(messageDeletions, []);
    assert.deepEqual(updates, []);
});

test('Rollback helper fails fast when deleting a targeted drift message fails', async () => {
    const { rollbackAgentRunDriftMessages } = await importFresh('src/scripts/tauritavern/agent/agent-run-message-rollback.js');
    const updates = [];

    await assert.rejects(
        () => rollbackAgentRunDriftMessages({
            runId: 'run-delete-fails',
            targets: [{ messageId: '0' }],
            script: installRollbackEventCapture({
                chat: [{ extra: { tauritavern: { agent: { runId: 'run-delete-fails', rollback: { strategy: 'deleteMessage' } } } } }],
                async deleteMessage() {
                    throw new Error('delete failed');
                },
            }, updates),
        }),
        /delete failed/,
    );
    assert.deepEqual(updates, []);
});
