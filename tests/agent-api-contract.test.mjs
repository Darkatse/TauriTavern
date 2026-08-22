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
        sha256: 'sha-19',
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
        options: {
            stream: false,
        },
    });

    assert.deepEqual(handle, { runId: 'run-model-target' });
    assert.equal(savedConnections.length, 1);
    assert.equal(savedConnections[0].auth.secretRef.id, 'secret-current');
    assert.ok(sequence.indexOf('llm_connections.save') < sequence.indexOf('start_agent_run'));
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






test('agent chat commit bridge detaches on partial success terminal event', async () => {
    const moduleUrl = pathToFileURL(path.join(REPO_ROOT, 'src/tauri/main/api/agent-chat-commit-bridge.js'));
    moduleUrl.search = `?case=partial-success-detach-${Date.now()}`;
    const { attachHostCommitBridge } = await import(moduleUrl.href);

    let listener = null;
    let stopped = false;
    attachHostCommitBridge({
        runId: 'run-partial',
        safeInvoke: async () => {},
        readWorkspaceFile: async () => {},
        subscribe(runId, handler) {
            assert.equal(runId, 'run-partial');
            listener = handler;
            return () => {
                stopped = true;
            };
        },
    });

    assert.equal(stopped, false);
    listener({ type: 'run_partial_success', payload: { preservedCommitCount: 1 } });
    assert.equal(stopped, true);
});

test('agent chat commit bridge runs generated output cleanup before saving', async () => {
    const moduleUrl = pathToFileURL(path.join(REPO_ROOT, 'src/tauri/main/api/agent-chat-commit-bridge.js'));
    moduleUrl.search = `?case=commit-cleanup-${Date.now()}`;
    const { attachHostCommitBridge } = await import(moduleUrl.href);
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

test('agent chat commit bridge retries failure resolution after workspace SHA mismatch', async () => {
    const moduleUrl = pathToFileURL(path.join(REPO_ROOT, 'src/tauri/main/api/agent-chat-commit-bridge.js'));
    moduleUrl.search = `?case=commit-recoverable-${Date.now()}`;
    const { attachHostCommitBridge } = await import(moduleUrl.href);
    const chatRef = { kind: 'character', characterId: 'Char', fileName: 'Chat.json' };
    installCurrentChatRef(chatRef);

    let listener = null;
    let resolveAttempt = 0;
    let resolveCommit;
    attachHostCommitBridge({
        runId: 'run-commit-recoverable',
        safeInvoke: async (command, args) => {
            if (command !== 'resolve_agent_chat_commit') return {};
            resolveAttempt += 1;
            if (resolveAttempt === 1) throw new Error('local IPC interrupted');
            resolveCommit(args);
            return {};
        },
        readWorkspaceFile: async () => workspaceFile('changed'),
        readModelTurn: async () => { throw new Error('reasoning projection unavailable'); },
        subscribe(_runId, handler) {
            listener = handler;
            return () => {};
        },
    });

    listener({ type: 'model_completed', payload: { invocationId: 'inv_root', round: 1, hasReasoning: true, reasoningChars: 9 } });
    const resolved = new Promise(resolve => { resolveCommit = resolve; });
    listener({
        type: 'chat_commit_requested',
        payload: agentCommitPayload(chatRef, {
            commitId: 'commit-recoverable',
            runId: 'run-commit-recoverable',
        }),
    });

    const result = await resolved;
    assert.equal(resolveAttempt, 2);
    assert.match(result.dto.error, /workspace content changed before commit/);
});

test('agent chat commit bridge preserves applied reasoning across a persistence retry', async () => {
    const moduleUrl = pathToFileURL(path.join(REPO_ROOT, 'src/tauri/main/api/agent-chat-commit-bridge.js'));
    moduleUrl.search = `?case=commit-append-cleanup-${Date.now()}`;
    const { attachHostCommitBridge } = await import(moduleUrl.href);
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
