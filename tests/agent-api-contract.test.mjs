import test from 'node:test';
import assert from 'node:assert/strict';
import path from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';
import { attachHostCommitBridge } from '../src/tauri/main/api/agent-chat-commit-bridge.js';

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

function installCurrentChatRef(chatRef) {
    ensureCustomEvent();
    globalThis.window = new EventTarget();
    globalThis.window.__TAURITAVERN__ = {
        api: {
            chat: {
                current: {
                    ref: () => chatRef,
                },
            },
        },
    };
}

function createFakeCommitScript(cleanUpMessage, saveCalls = []) {
    const script = {
        chat: [],
        cleanUpMessage,
        async saveReply({ type, getMessage, reasoning = '' }) {
            saveCalls.push({ type, getMessage, reasoning });
            if (type === 'appendFinal') {
                const message = script.chat[script.chat.length - 1];
                message.mes = getMessage;
                message.extra.reasoning += reasoning;
                message.swipes[message.swipe_id] = getMessage;
                return { type, getMessage };
            }

            script.chat.push({
                mes: getMessage,
                extra: { reasoning },
                swipe_id: 0,
                swipes: [getMessage],
                swipe_info: [{ extra: {} }],
            });
            return { type, getMessage };
        },
    };
    return script;
}

function createFakeStreamingCommitScript() {
    const saveCalls = [];
    const renders = [];
    const events = [];
    const script = {
        chat: [],
        event_types: {
            MESSAGE_RECEIVED: 'message_received',
            CHARACTER_MESSAGE_RENDERED: 'character_message_rendered',
        },
        eventSource: {
            async emit(...args) {
                events.push(args);
            },
        },
        cleanUpMessage({ getMessage }) {
            return String(getMessage ?? '');
        },
        async saveReply({ type, getMessage, reasoning = '', fromStreaming = false }) {
            saveCalls.push({ type, getMessage, reasoning, fromStreaming });
            if (type === 'appendFinal') {
                const message = script.chat.at(-1);
                message.mes = getMessage;
                message.extra.reasoning += reasoning;
                message.swipes[message.swipe_id] = getMessage;
            } else if (type === 'swipe' && script.chat.length > 0) {
                const message = script.chat.at(-1);
                message.mes = getMessage;
                message.extra.reasoning = reasoning;
                message.swipes[message.swipe_id] = getMessage;
                message.swipe_info[message.swipe_id] = {
                    extra: structuredClone(message.extra),
                };
            } else {
                script.chat.push({
                    mes: getMessage,
                    extra: { reasoning },
                    swipe_id: 0,
                    swipes: [getMessage],
                    swipe_info: [{ extra: { reasoning } }],
                });
            }
            if (!fromStreaming) {
                const messageId = script.chat.length - 1;
                await script.eventSource.emit(script.event_types.MESSAGE_RECEIVED, messageId, type);
                await script.eventSource.emit(script.event_types.CHARACTER_MESSAGE_RENDERED, messageId, type);
            }
            return { type, getMessage };
        },
        syncMesToSwipe(messageId) {
            const message = script.chat[messageId];
            message.swipes[message.swipe_id] = message.mes;
            message.swipe_info[message.swipe_id].extra = structuredClone(message.extra);
            return true;
        },
        updateMessageBlock(messageId, message, options) {
            renders.push({ messageId, text: message.mes, options });
        },
    };
    return { script, saveCalls, renders, events };
}

function workspaceFile(text, pathName = 'output/main.md') {
    return {
        path: pathName,
        text,
        chars: text.length,
        words: text.trim() ? text.trim().split(/\s+/).length : 0,
        sha256: `sha-${text.length}`,
    };
}

function agentCommitPayload(chatRef, overrides = {}) {
    return {
        commitId: 'commit-1',
        runId: 'run-commit',
        workspaceId: 'workspace-1',
        stableChatId: 'stable-1',
        chatRef,
        generationType: 'normal',
        profileId: 'default-writer',
        persistBaseStateId: null,
        path: 'output/main.md',
        mode: 'replace',
        isExplicit: false,
        sha256: 'sha-19',
        ...overrides,
    };
}

function liveWriteCall(content, overrides = {}) {
    return {
        toolId: 'builtin:workspace.write_file',
        invocationId: 'inv_root',
        invocationExitPolicy: 'run_finish_allowed',
        toolCallIndex: 0,
        path: 'output/main.md',
        content,
        contentWords: 0,
        ...overrides,
    };
}

async function installHarness(options = {}) {
    const calls = [];
    ensureCustomEvent();
    globalThis.window = new EventTarget();
    globalThis.window.__TAURITAVERN__ = { api: {} };
    const safeInvoke = options.safeInvoke || (async (command, args) => {
        calls.push({ command, args });
        return { command, args };
    });

    const { installAgentApi } = await import(pathToFileURL(path.join(REPO_ROOT, 'src/tauri/main/api/agent.js')));
    installAgentApi({
        safeInvoke,
    });

    return {
        calls,
        agent: globalThis.window.__TAURITAVERN__.api.agent,
    };
}

test('Agent run options preserve an explicit stream override and preserve omission', async () => {
    const { normalizeAgentRunOptions } = await import(pathToFileURL(path.join(
        REPO_ROOT,
        'src/tauri/main/api/agent-run-options.js',
    )));

    assert.deepEqual(normalizeAgentRunOptions(undefined), {});
    assert.deepEqual(normalizeAgentRunOptions({ stream: true }), { stream: true });
    assert.deepEqual(normalizeAgentRunOptions({ stream: false }), { stream: false });
    assert.throws(
        () => normalizeAgentRunOptions({ stream: 'true' }),
        /agent\.stream_invalid/,
    );
});

test('Agent live projection subscription owns Channel callbacks and detaches idempotently', async () => {
    const { createAgentRunLiveSubscribe } = await import(pathToFileURL(path.join(
        REPO_ROOT,
        'src/tauri/main/api/agent-run-live-subscription.js',
    )));
    let onmessage;
    let resolveInvoke;
    const invokeCompletion = new Promise((resolve) => {
        resolveInvoke = resolve;
    });
    const calls = [];
    const updates = [];
    const subscribe = createAgentRunLiveSubscribe({
        safeInvoke(command, args) {
            calls.push({ command, args });
            return invokeCompletion;
        },
        channelFactory(handler) {
            onmessage = handler;
            return { kind: 'test-channel' };
        },
    });

    const unsubscribe = subscribe(' run-live ', updates.push.bind(updates));
    assert.equal(calls[0].command, 'subscribe_agent_run_live_projection');
    assert.deepEqual(calls[0].args, {
        dto: { runId: 'run-live' },
        channel: { kind: 'test-channel' },
    });
    onmessage({ type: 'snapshot', calls: [] });
    unsubscribe();
    unsubscribe();
    onmessage({ type: 'remove', invocationId: 'inv_root', toolCallIndex: 0 });
    resolveInvoke();
    await Promise.resolve();
    assert.deepEqual(updates, [{ type: 'snapshot', calls: [] }]);
});

test('Agent live projection subscription reports command rejection', async () => {
    const { createAgentRunLiveSubscribe } = await import(pathToFileURL(path.join(
        REPO_ROOT,
        'src/tauri/main/api/agent-run-live-subscription.js',
    )));
    const errors = [];
    const channelFactory = () => ({});

    createAgentRunLiveSubscribe({
        safeInvoke: async () => {
            throw new Error('channel failed');
        },
        channelFactory,
    })('run-error', () => {}, { onError: error => errors.push(error.message) });

    await Promise.resolve();
    await Promise.resolve();
    assert.deepEqual(errors, ['channel failed']);
});


test('api.agent.profiles publishes profile change events after successful mutations', async () => {
    const { agent } = await installHarness();
    const { subscribeAgentProfilesChanged } = await import(pathToFileURL(path.join(
        REPO_ROOT,
        'src/scripts/tauritavern/agent/agent-profile-events.js',
    )));
    const events = [];
    const unsubscribe = subscribeAgentProfilesChanged(() => {
        events.push('changed');
    });

    await agent.profiles.save({ profile: { id: 'writer' } });
    await agent.profiles.retargetPresetRefs({
        from: { apiId: 'openai', name: 'Old Preset' },
        to: { apiId: 'openai', name: 'New Preset' },
    });
    await agent.profiles.delete('writer');
    await agent.profiles.repairFile({ profileId: 'writer', action: 'delete' });
    unsubscribe();

    assert.deepEqual(events, ['changed', 'changed', 'changed', 'changed']);
});





test('api.agent.startRunWithPromptSnapshot refreshes Model Target LLM connection before starting run', async () => {
    const sequence = [];
    const savedConnections = [];
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
        },
    };
    const { agent } = await installHarness({
        safeInvoke: async (command, args) => {
            sequence.push(command);
            if (command === 'load_agent_profile') {
                assert.equal(args.dto.profileId, 'writer');
                return {
                    profile: {
                        model: {
                            mode: 'connectionRef',
                            connectionRef: 'model-target-writer-target',
                            modelId: 'claude-3-7-sonnet',
                        },
                        preset: {
                            mode: 'ref',
                        },
                        run: {
                            presentation: 'foreground',
                            stream: true,
                        },
                    },
                };
            }
            if (command === 'start_agent_run') {
                return { runId: 'run-model-target' };
            }
            if (command === 'read_agent_run_events') {
                return {
                    events: [{
                        id: 'evt-terminal',
                        seq: 1,
                        runId: 'run-model-target',
                        type: 'run_completed',
                        payload: {},
                    }],
                };
            }
            return {};
        },
    });
    globalThis.window.__TAURI__ = {
        core: { Channel: class { constructor(onmessage) { this.onmessage = onmessage; } } },
    };
    globalThis.window.__TAURITAVERN__.api.llmConnections = {
        async save({ connection }) {
            sequence.push('llm_connections.save');
            savedConnections.push(connection);
        },
    };
    globalThis.window.SillyTavern = {
        getContext: () => ({
            extensionSettings: {
                connectionManager: {
                    modelTargets: [currentTarget],
                },
            },
        }),
    };

    const handle = await agent.startRunWithPromptSnapshot({
        chatRef: { kind: 'character', characterId: 'char-1', fileName: 'Char.json' },
        stableChatId: 'stable-chat-1',
        generationType: 'normal',
        profileId: 'writer',
        promptSnapshot: {
            contextPolicy: {},
            chatCompletionPayload: {
                messages: [],
            },
        },
    });

    assert.deepEqual(handle, { runId: 'run-model-target' });
    assert.equal(savedConnections.length, 1);
    assert.equal(savedConnections[0].auth.secretRef.id, 'secret-current');
    assert.ok(sequence.indexOf('llm_connections.save') < sequence.indexOf('start_agent_run'));
    await waitFor(() => sequence.includes('subscribe_agent_run_live_projection'));
    await waitFor(() => sequence.includes('read_agent_run_events'));
});


test('api.agent.submitGuidance forwards camelCase DTO and fails fast on invalid input', async () => {
    const { calls, agent } = await installHarness();

    await agent.submitGuidance({
        runId: ' run_guidance ',
        text: '  Keep the ending restrained.  ',
        clientGuidanceId: ' client-guidance-1 ',
    });
    await agent.submitGuidance({
        runId: 'run_guidance',
        text: 'No client id.',
    });

    assert.deepEqual(calls, [
        {
            command: 'submit_agent_run_guidance',
            args: {
                dto: {
                    runId: 'run_guidance',
                    text: 'Keep the ending restrained.',
                    clientGuidanceId: 'client-guidance-1',
                },
            },
        },
        {
            command: 'submit_agent_run_guidance',
            args: {
                dto: {
                    runId: 'run_guidance',
                    text: 'No client id.',
                },
            },
        },
    ]);

    await assert.rejects(
        () => agent.submitGuidance(null),
        /Agent submitGuidance input must be an object/,
    );
    await assert.rejects(
        () => agent.submitGuidance({ runId: '', text: 'hello' }),
        /runId is required/,
    );
    await assert.rejects(
        () => agent.submitGuidance({ runId: 'run_guidance', text: '   ' }),
        /guidance text is required/,
    );
});


test('api.agent.listRuns fails fast on invalid history filters', async () => {
    const { calls, agent } = await installHarness();

    await assert.rejects(
        () => agent.listRuns(null),
        /Agent listRuns input must be an object/,
    );
    await assert.rejects(
        () => agent.listRuns({ chatRef: 'bad' }),
        /chatRef must be an object/,
    );
    await assert.rejects(
        () => agent.listRuns({ statuses: 'completed' }),
        /statuses must be an array/,
    );
    await assert.rejects(
        () => agent.listRuns({ statuses: ['completed', ''] }),
        /statuses contains an empty status/,
    );
    await assert.rejects(
        () => agent.listRuns({ statuses: ['done'] }),
        /unknown agent run status/,
    );
    await assert.rejects(
        () => agent.listRuns({ before: { createdAt: '2026-01-02T03:04:05.000Z' } }),
        /before.runId is required/,
    );
    await assert.rejects(
        () => agent.listRuns({ before: { runId: 'run_a', createdAt: 'not-a-date' } }),
        /before.createdAt must be a valid timestamp/,
    );
    await assert.rejects(
        () => agent.listRuns({ before: { runId: 'run_a', createdAt: new Date(Number.NaN) } }),
        /before.createdAt must be a valid timestamp/,
    );
    await assert.rejects(
        () => agent.listRuns({ limit: 0 }),
        /limit must be an integer between 1 and 200/,
    );
    assert.deepEqual(calls, []);
});

test('agent live write keeps one real partial chat message and saves it on failure', async () => {
    const chatRef = { kind: 'character', characterId: 'Char', fileName: 'Chat.json' };
    installCurrentChatRef(chatRef);
    const originalRequestAnimationFrame = globalThis.requestAnimationFrame;
    const originalCancelAnimationFrame = globalThis.cancelAnimationFrame;
    let suspendFrames = false;
    let cancelledFrames = 0;
    globalThis.requestAnimationFrame = callback => {
        if (!suspendFrames) queueMicrotask(() => callback(0));
        return 1;
    };
    globalThis.cancelAnimationFrame = () => { cancelledFrames += 1; };

    try {
        const { script, events, renders } = createFakeStreamingCommitScript();
        let durableListener = null;
        let liveListener = null;
        let durableStopped = false;
        let liveStopped = false;
        let persistCount = 0;
        attachHostCommitBridge({
            runId: 'run-live-partial',
            chatRef,
            stableChatId: 'stable-live-partial',
            generationType: 'normal',
            safeInvoke: async () => {},
            readWorkspaceFile: async () => {},
            subscribe(_runId, handler) {
                durableListener = handler;
                return () => { durableStopped = true; };
            },
            subscribeLiveProjection(_runId, handler) {
                let active = true;
                liveListener = update => { if (active) handler(update); };
                return () => {
                    active = false;
                    liveStopped = true;
                };
            },
            loadScript: async () => script,
            persistChat: async () => { persistCount += 1; },
        });

        liveListener({
            type: 'replace',
            call: liveWriteCall('partial'),
        });
        await waitFor(() => script.chat[0]?.mes === 'partial');
        const message = script.chat[0];
        assert.equal(message.extra.tauritavern, undefined);

        suspendFrames = true;
        liveListener({
            type: 'replace',
            call: liveWriteCall('child content', {
                invocationId: 'inv_background_child',
                invocationExitPolicy: 'task_return_required',
                path: 'output/child.md',
            }),
        });
        liveListener({
            type: 'append',
            invocationId: 'inv_root',
            toolCallIndex: 0,
            field: 'content',
            text: ' answer',
            wordDelta: 1,
        });
        liveListener({ type: 'remove', invocationId: 'inv_root', toolCallIndex: 0 });
        liveListener({
            type: 'replace',
            call: liveWriteCall('handoff', {
                invocationId: 'inv_handoff_before_journal_poll',
            }),
        });
        liveListener({
            type: 'append',
            invocationId: 'inv_handoff_before_journal_poll',
            toolCallIndex: 0,
            field: 'content',
            text: ' answer',
            wordDelta: 1,
        });

        durableListener({ type: 'run_failed', payload: {} });
        await waitFor(() => persistCount === 1);
        assert.equal(cancelledFrames, 1);
        assert.equal(script.chat.length, 1);
        assert.deepEqual(renders.at(-1).options, { transient: true });
        assert.equal(script.chat[0], message);
        assert.equal(message.mes, 'handoff answer');
        assert.equal(message.extra.tauritavern, undefined);
        assert.deepEqual(events.slice(-2), [
            ['message_received', 0, 'normal'],
            ['character_message_rendered', 0, 'normal'],
        ]);
        assert.equal(durableStopped, true);
        assert.equal(liveStopped, true);
    } finally {
        globalThis.requestAnimationFrame = originalRequestAnimationFrame;
        globalThis.cancelAnimationFrame = originalCancelAnimationFrame;
    }
});

test('agent live swipe keeps prior commit metadata on the prior swipe only', async () => {
    const chatRef = { kind: 'character', characterId: 'Char', fileName: 'Chat.json' };
    installCurrentChatRef(chatRef);
    const originalRequestAnimationFrame = globalThis.requestAnimationFrame;
    globalThis.requestAnimationFrame = callback => {
        queueMicrotask(() => callback(0));
        return 1;
    };

    try {
        const { script } = createFakeStreamingCommitScript();
        const oldExtra = {
            reasoning: '',
            tauritavern: { agent: { runId: 'run-old', commitId: 'commit-old' } },
        };
        script.chat.push({
            mes: 'old answer',
            extra: structuredClone(oldExtra),
            swipe_id: 1,
            swipes: ['old answer'],
            swipe_info: [{ extra: structuredClone(oldExtra) }],
        });
        let liveListener = null;
        attachHostCommitBridge({
            runId: 'run-live-swipe',
            chatRef,
            stableChatId: 'stable-live-swipe',
            generationType: 'swipe',
            safeInvoke: async () => {},
            readWorkspaceFile: async () => {},
            subscribe() { return () => {}; },
            subscribeLiveProjection(_runId, handler) {
                liveListener = handler;
                return () => {};
            },
            loadScript: async () => script,
            persistChat: async () => {},
        });

        liveListener({
            type: 'replace',
            call: liveWriteCall('new swipe'),
        });
        await waitFor(() => script.chat[0].mes === 'new swipe');
        assert.equal(script.chat.length, 1);
        assert.equal(script.chat[0].extra.tauritavern, undefined);
        assert.equal(script.chat[0].swipe_info[1].extra.tauritavern, undefined);
        assert.equal(script.chat[0].swipe_info[0].extra.tauritavern.agent.runId, 'run-old');
    } finally {
        globalThis.requestAnimationFrame = originalRequestAnimationFrame;
    }
});

test('agent live write reuses auto checkpoints and stops after the first explicit commit', async () => {
    const chatRef = { kind: 'character', characterId: 'Char', fileName: 'Chat.json' };
    installCurrentChatRef(chatRef);
    const originalRequestAnimationFrame = globalThis.requestAnimationFrame;
    globalThis.requestAnimationFrame = callback => {
        queueMicrotask(() => callback(0));
        return 1;
    };

    try {
        const { script } = createFakeStreamingCommitScript();
        const files = [workspaceFile('patched'), workspaceFile('final')];
        const resolutions = [];
        let durableListener = null;
        let liveListener = null;
        let liveStopped = false;
        let persistCount = 0;
        attachHostCommitBridge({
            runId: 'run-live-commit',
            chatRef,
            stableChatId: 'stable-live-commit',
            generationType: 'normal',
            safeInvoke: async (command, args) => {
                if (command === 'resolve_agent_chat_commit') resolutions.push(args.dto);
            },
            readWorkspaceFile: async () => files.shift(),
            subscribe(_runId, handler) {
                durableListener = handler;
                return () => {};
            },
            subscribeLiveProjection(_runId, handler) {
                let active = true;
                liveListener = update => { if (active) handler(update); };
                return () => {
                    active = false;
                    liveStopped = true;
                };
            },
            loadScript: async () => script,
            persistChat: async () => { persistCount += 1; },
        });

        const replace = content => liveListener({
            type: 'replace',
            call: liveWriteCall(content),
        });
        replace('draft');
        await waitFor(() => script.chat[0]?.mes === 'draft');
        const message = script.chat[0];

        durableListener({
            type: 'chat_commit_requested',
            payload: agentCommitPayload(chatRef, {
                runId: 'run-live-commit',
                commitId: 'commit-auto',
                stableChatId: 'stable-live-commit',
                sha256: 'sha-7',
                isExplicit: false,
            }),
        });
        await waitFor(() => resolutions.length === 1);
        assert.equal(message.mes, 'patched');
        assert.equal(liveStopped, false);

        replace('after checkpoint');
        await waitFor(() => message.mes === 'after checkpoint');
        assert.equal(script.chat[0], message);
        assert.equal(script.chat.length, 1);
        assert.equal(message.extra.tauritavern.agent.commitId, 'commit-auto');
        assert.equal(message.extra.tauritavern.agent.artifacts[0].sha256, 'sha-7');

        durableListener({
            type: 'chat_commit_requested',
            payload: agentCommitPayload(chatRef, {
                runId: 'run-live-commit',
                commitId: 'commit-explicit',
                stableChatId: 'stable-live-commit',
                sha256: 'sha-5',
                isExplicit: true,
            }),
        });
        await waitFor(() => resolutions.length === 2);
        assert.equal(message.mes, 'final');
        assert.equal(liveStopped, true);
        assert.equal(persistCount, 2);

        replace('ignored');
        await Promise.resolve();
        await Promise.resolve();
        assert.equal(message.mes, 'final');
    } finally {
        globalThis.requestAnimationFrame = originalRequestAnimationFrame;
    }
});

test('agent live write emits generated-message events once when commit persistence fails', async () => {
    const chatRef = { kind: 'character', characterId: 'Char', fileName: 'Chat.json' };
    installCurrentChatRef(chatRef);
    const originalRequestAnimationFrame = globalThis.requestAnimationFrame;
    globalThis.requestAnimationFrame = callback => {
        queueMicrotask(() => callback(0));
        return 1;
    };

    try {
        const { script, events } = createFakeStreamingCommitScript();
        const resolutions = [];
        let durableListener = null;
        let liveListener = null;
        let persistAttempts = 0;
        attachHostCommitBridge({
            runId: 'run-live-persist-failure',
            chatRef,
            stableChatId: 'stable-live-persist-failure',
            generationType: 'normal',
            safeInvoke: async (command, args) => {
                if (command === 'resolve_agent_chat_commit') resolutions.push(args.dto);
            },
            readWorkspaceFile: async () => workspaceFile('draft'),
            subscribe(_runId, handler) {
                durableListener = handler;
                return () => {};
            },
            subscribeLiveProjection(_runId, handler) {
                liveListener = handler;
                return () => {};
            },
            loadScript: async () => script,
            persistChat: async () => {
                persistAttempts += 1;
                if (persistAttempts === 1) throw new Error('chat persistence failed');
            },
        });

        liveListener({
            type: 'replace',
            call: liveWriteCall('draft'),
        });
        await waitFor(() => script.chat[0]?.mes === 'draft');
        durableListener({
            type: 'chat_commit_requested',
            payload: agentCommitPayload(chatRef, {
                runId: 'run-live-persist-failure',
                commitId: 'commit-persist-failure',
                stableChatId: 'stable-live-persist-failure',
                sha256: 'sha-5',
            }),
        });
        await waitFor(() => resolutions.length === 1);
        assert.match(resolutions[0].error, /chat persistence failed/);
        assert.equal(events.length, 2);

        durableListener({ type: 'run_failed', payload: {} });
        await waitFor(() => persistAttempts === 2);
        assert.equal(events.length, 2);
        assert.equal(script.chat[0].extra.tauritavern, undefined);
    } finally {
        globalThis.requestAnimationFrame = originalRequestAnimationFrame;
    }
});

test('agent chat commit bridge runs generated output cleanup before saving', async () => {
    const chatRef = { kind: 'character', characterId: 'Char', fileName: 'Chat.json' };
    installCurrentChatRef(chatRef);

    const cleanups = [];
    const saveCalls = [];
    const script = createFakeCommitScript((options) => {
        cleanups.push(options);
        return options.getMessage.replace(/^[\s\S]*?(<content>)/, '$1');
    }, saveCalls);
    let listener = null;
    const resolutions = [];
    const workspaceReads = [];
    attachHostCommitBridge({
        runId: 'run-commit-cleanup',
        safeInvoke: async (command, args) => {
            if (command === 'resolve_agent_chat_commit') {
                resolutions.shift()(args);
            }
            return {};
        },
        readWorkspaceFile: async (input) => {
            workspaceReads.push(input);
            return workspaceFile('debug <content>real');
        },
        subscribe(runId, handler) {
            assert.equal(runId, 'run-commit-cleanup');
            listener = handler;
            return () => {};
        },
        loadScript: async () => script,
        persistChat: async () => {},
    });

    const resolved = new Promise(resolve => resolutions.push(resolve));
    listener({
        type: 'chat_commit_requested',
        payload: agentCommitPayload(chatRef, {
            commitId: 'commit-cleanup',
            runId: 'run-commit-cleanup',
        }),
    });
    const result = await resolved;
    assert.equal(result.dto.error, undefined);

    assert.deepEqual(cleanups, [{
        getMessage: 'debug <content>real',
        isImpersonate: false,
        isContinue: false,
        displayIncompleteSentences: false,
    }]);
    assert.deepEqual(workspaceReads, [{
        runId: 'run-commit-cleanup',
        path: 'output/main.md',
    }]);
    assert.deepEqual(saveCalls, [{ type: 'normal', getMessage: '<content>real', reasoning: '' }]);
    assert.equal(script.chat[0].mes, '<content>real');
});

test('agent chat commit bridge preserves applied reasoning across a persistence retry', async () => {
    const chatRef = { kind: 'character', characterId: 'Char', fileName: 'Chat.json' };
    installCurrentChatRef(chatRef);

    const cleanups = [];
    const saveCalls = [];
    const script = createFakeCommitScript((options) => {
        cleanups.push(options.getMessage);
        return options.getMessage.includes('<content>')
            ? options.getMessage.replace(/^[\s\S]*?(<content>)/, '$1')
            : options.getMessage;
    }, saveCalls);
    const files = [workspaceFile('debug '), workspaceFile('<content>real')];
    const resolutions = [];
    const modelTurnReads = [];
    let persistAttempts = 0;
    let listener = null;
    attachHostCommitBridge({
        runId: 'run-commit-append-cleanup',
        safeInvoke: async (command, args) => {
            if (command === 'resolve_agent_chat_commit') {
                resolutions.shift()(args);
            }
            return {};
        },
        readWorkspaceFile: async () => files.shift(),
        readModelTurn: async (input) => {
            modelTurnReads.push(input);
            return {
                reasoning: [{
                    text: input.round === 1 ? 'first thought' : 'second thought',
                    totalChars: input.round === 1 ? 13 : 14,
                    truncated: false,
                }],
            };
        },
        subscribe(runId, handler) {
            assert.equal(runId, 'run-commit-append-cleanup');
            listener = handler;
            return () => {};
        },
        loadScript: async () => script,
        persistChat: async () => {
            persistAttempts += 1;
            if (persistAttempts === 1) throw new Error('chat persistence failed');
        },
    });

    listener({ type: 'agent_invocation_created', payload: { invocationId: 'inv_child', exitPolicy: 'task_return_required' } });
    listener({ type: 'model_completed', payload: { invocationId: 'inv_child', round: 1, hasReasoning: true, reasoningChars: 7 } });
    listener({ type: 'model_completed', payload: { invocationId: 'inv_root', round: 1, hasReasoning: true, reasoningChars: 13 } });
    const firstResolved = new Promise(resolve => resolutions.push(resolve));
    listener({
        type: 'chat_commit_requested',
        payload: agentCommitPayload(chatRef, {
            commitId: 'commit-append-1',
            runId: 'run-commit-append-cleanup',
            mode: 'append',
            sha256: 'sha-6',
        }),
    });
    const firstResult = await firstResolved;
    assert.match(firstResult.dto.error, /chat persistence failed/);
    assert.equal(script.chat[0].extra.tauritavern, undefined);

    listener({ type: 'model_completed', payload: { invocationId: 'inv_root', round: 2, hasReasoning: true, reasoningChars: 14 } });
    const secondResolved = new Promise(resolve => resolutions.push(resolve));
    listener({
        type: 'chat_commit_requested',
        payload: agentCommitPayload(chatRef, {
            commitId: 'commit-append-2',
            runId: 'run-commit-append-cleanup',
            mode: 'append',
            sha256: 'sha-13',
        }),
    });
    const secondResult = await secondResolved;
    assert.equal(secondResult.dto.error, undefined);
    assert.equal(persistAttempts, 2);

    assert.deepEqual(cleanups, ['debug ', 'debug <content>real']);
    assert.deepEqual(saveCalls, [
        { type: 'normal', getMessage: 'debug ', reasoning: 'first thought' },
        { type: 'appendFinal', getMessage: '<content>real', reasoning: '\n\nsecond thought' },
    ]);
    assert.equal(script.chat[0].mes, '<content>real');
    assert.deepEqual(modelTurnReads, [
        { runId: 'run-commit-append-cleanup', invocationId: 'inv_root', round: 1, maxChars: 13 },
        { runId: 'run-commit-append-cleanup', invocationId: 'inv_root', round: 2, maxChars: 14 },
    ]);
    assert.equal(script.chat[0].extra.reasoning, 'first thought\n\nsecond thought');
    assert.deepEqual(
        script.chat[0].extra.tauritavern.agent.commits.map(commit => commit.commitId),
        ['commit-append-2'],
    );
});


test('shared agent run event subscription fans out over one backend poller', async () => {
    const moduleUrl = pathToFileURL(path.join(REPO_ROOT, 'src/tauri/main/api/agent-run-event-subscription.js'));
    moduleUrl.search = `?case=shared-run-event-subscription-${Date.now()}`;
    const { createSharedRunEventSubscribe } = await import(moduleUrl.href);
    const firstEvents = [];
    const secondEvents = [];
    const firstErrors = [];
    const secondErrors = [];
    let pollStarts = 0;
    let pollStops = 0;
    let dispatch = null;
    let dispatchError = null;

    const subscribe = createSharedRunEventSubscribe('run-shared', (runId, handler, options = {}) => {
        pollStarts += 1;
        assert.equal(runId, 'run-shared');
        dispatch = handler;
        dispatchError = options.onError;
        return () => {
            pollStops += 1;
        };
    });

    const stopFirst = subscribe('run-shared', event => {
        firstEvents.push(event.type);
    }, {
        onError(error) {
            firstErrors.push(String(error?.message ?? error));
        },
    });
    const stopSecond = subscribe('run-shared', event => {
        secondEvents.push(event.type);
    }, {
        onError(error) {
            secondErrors.push(String(error?.message ?? error));
        },
    });

    assert.equal(pollStarts, 1);
    dispatch({ type: 'context_assembled' });
    dispatchError(new Error('poll failed'));
    assert.deepEqual(firstEvents, ['context_assembled']);
    assert.deepEqual(secondEvents, ['context_assembled']);
    assert.deepEqual(firstErrors, ['poll failed']);
    assert.deepEqual(secondErrors, ['poll failed']);

    stopFirst();
    assert.equal(pollStops, 0);
    dispatch({ type: 'prompt_assembly_requested' });
    assert.deepEqual(firstEvents, ['context_assembled']);
    assert.deepEqual(secondEvents, ['context_assembled', 'prompt_assembly_requested']);

    stopSecond();
    assert.equal(pollStops, 1);
    assert.throws(
        () => subscribe('another-run', () => {}),
        /agent\.subscribe_run_mismatch/,
    );
});

async function waitFor(predicate) {
    for (let i = 0; i < 20; i += 1) {
        if (predicate()) {
            return;
        }
        await new Promise(resolve => setTimeout(resolve, 0));
    }
    assert.fail('condition was not met');
}
