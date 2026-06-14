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

test('api.agent.profiles forwards profile commands with camelCase DTOs', async () => {
    const { calls, agent } = await installHarness();
    const profile = {
        schemaVersion: 1,
        kind: 'tauritavern.agentProfile',
        id: 'writer',
    };

    assert.ok(agent.profiles);
    await agent.profiles.list();
    await agent.profiles.load({ profileId: 'writer' });
    await agent.profiles.diagnose({ profileId: 'writer' });
    await agent.profiles.resolveSystemPrompt({ profileId: 'writer' });
    await agent.profiles.retargetPresetRefs({
        from: { apiId: 'openai', name: 'Old Preset' },
        to: { apiId: 'openai', name: 'New Preset' },
    });
    await agent.profiles.save({ profile });
    await agent.profiles.delete('writer');
    await agent.profiles.repairFile({ profileId: 'writer', action: 'normalizeIdentity' });

    assert.deepEqual(calls, [
        { command: 'list_agent_profiles', args: undefined },
        { command: 'load_agent_profile', args: { dto: { profileId: 'writer' } } },
        { command: 'diagnose_agent_profile', args: { dto: { profileId: 'writer' } } },
        { command: 'resolve_agent_system_prompt', args: { dto: { profileId: 'writer' } } },
        {
            command: 'retarget_agent_profile_preset_refs',
            args: {
                dto: {
                    from: { apiId: 'openai', name: 'Old Preset' },
                    to: { apiId: 'openai', name: 'New Preset' },
                },
            },
        },
        { command: 'save_agent_profile', args: { dto: { profile } } },
        { command: 'delete_agent_profile', args: { dto: { profileId: 'writer' } } },
        { command: 'repair_agent_profile_file', args: { dto: { profileId: 'writer', action: 'normalizeIdentity' } } },
    ]);
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

test('api.agent.profiles fails fast on invalid profile inputs', async () => {
    const { agent } = await installHarness();

    await assert.rejects(
        () => agent.profiles.load({ profileId: '' }),
        /profileId is required/,
    );
    await assert.rejects(
        () => agent.profiles.delete(''),
        /profileId is required/,
    );
    await assert.rejects(
        () => agent.profiles.save(null),
        /profile must be an object/,
    );
    await assert.rejects(
        () => agent.profiles.retargetPresetRefs({ from: { apiId: 'openai' }, to: { apiId: 'openai', name: 'New' } }),
        /from requires apiId and name/,
    );
    await assert.rejects(
        () => agent.profiles.repairFile({ profileId: 'writer', action: 'archive' }),
        /repair action must be delete or normalizeIdentity/,
    );
});

test('api.agent.tools lists canonical tool specs', async () => {
    const { calls, agent } = await installHarness();

    assert.ok(agent.tools);
    await agent.tools.list();

    assert.deepEqual(calls, [
        { command: 'list_agent_tool_specs', args: undefined },
    ]);
});

test('api.agent.promptAssembly prepares backend broker requests', async () => {
    const { calls, agent } = await installHarness();
    const frozenRunInputSnapshot = {
        schemaVersion: 1,
        kind: 'tauritavern.agentFrozenRunInputSnapshot',
        generationType: 'swipe',
        promptInputs: { type: 'swipe', messages: [] },
        worldInfoActivation: { entries: [] },
        macroContext: { names: { user: 'User', char: 'Char' } },
    };

    assert.ok(agent.promptAssembly);
    await agent.promptAssembly.prepare({
        profileId: 'writer',
        generationType: 'swipe',
        frozenRunInputSnapshot,
        jsonSchema: { type: 'object' },
    });

    assert.deepEqual(calls, [
        {
            command: 'prepare_agent_prompt_assembly',
            args: {
                dto: {
                    profileId: 'writer',
                    generationType: 'swipe',
                    frozenRunInputSnapshot,
                    jsonSchema: { type: 'object' },
                },
            },
        },
    ]);
});

test('api.agent.readEvents requests timeline projection only when asked', async () => {
    const { calls, agent } = await installHarness();

    await agent.readEvents({ runId: 'run-1', afterSeq: 12, limit: 20 });
    await agent.readEvents({
        runId: 'run-1',
        beforeSeq: 200,
        limit: 50,
        includeTimelineProjection: true,
    });

    assert.deepEqual(calls, [
        {
            command: 'read_agent_run_events',
            args: {
                dto: {
                    runId: 'run-1',
                    afterSeq: 12,
                    beforeSeq: undefined,
                    limit: 20,
                },
            },
        },
        {
            command: 'read_agent_run_events',
            args: {
                dto: {
                    runId: 'run-1',
                    afterSeq: undefined,
                    beforeSeq: 200,
                    limit: 50,
                    includeTimelineProjection: true,
                },
            },
        },
    ]);
});

test('api.agent.listRuns forwards run history filters with camelCase DTOs', async () => {
    const { calls, agent } = await installHarness();
    const chatRef = { kind: 'character', characterId: 'char-1', fileName: 'Char.json' };

    await agent.listRuns();
    await agent.listRuns({
        chatRef,
        stableChatId: ' stable_1 ',
        statuses: ['completed', 'failed', 'completed'],
        before: {
            createdAt: '2026-01-02T11:04:05+08:00',
            runId: ' run_b ',
        },
        limit: 25,
    });

    assert.deepEqual(calls, [
        {
            command: 'list_agent_runs',
            args: { dto: {} },
        },
        {
            command: 'list_agent_runs',
            args: {
                dto: {
                    chatRef,
                    stableChatId: 'stable_1',
                    statuses: ['completed', 'failed'],
                    before: {
                        createdAt: '2026-01-02T03:04:05.000Z',
                        runId: 'run_b',
                    },
                    limit: 25,
                },
            },
        },
    ]);
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

test('api.agent.retention forwards settings and prune contracts', async () => {
    const calls = [];
    const { agent } = await installHarness({
        safeInvoke: async (command, args) => {
            calls.push({ command, args });
            if (command === 'get_tauritavern_settings') {
                return {
                    agent: {
                        retention: {
                            auto_prune_enabled: true,
                            keep_recent_terminal_runs: 100,
                            keep_full_recent_runs: 20,
                        },
                    },
                };
            }
            if (command === 'update_tauritavern_settings') {
                return {
                    agent: {
                        retention: {
                            auto_prune_enabled: args.dto.agent.retention.auto_prune_enabled,
                            keep_recent_terminal_runs: args.dto.agent.retention.keep_recent_terminal_runs,
                            keep_full_recent_runs: args.dto.agent.retention.keep_full_recent_runs,
                        },
                    },
                };
            }
            return { ok: true };
        },
    });

    assert.deepEqual(await agent.retention.readSettings(), {
        autoPruneEnabled: true,
        keepRecentTerminalRuns: 100,
        keepFullRecentRuns: 20,
    });
    assert.deepEqual(await agent.retention.updateSettings({
        autoPruneEnabled: false,
        keepRecentTerminalRuns: '80',
        keepFullRecentRuns: 12,
    }), {
        autoPruneEnabled: false,
        keepRecentTerminalRuns: 80,
        keepFullRecentRuns: 12,
    });
    await agent.retention.planPrune({
        retention: {
            keepRecentTerminalRuns: 80,
            keepFullRecentRuns: 12,
        },
        detailLimit: 8,
    });
    await agent.retention.applyPrune({
        retention: {
            keepRecentTerminalRuns: 80,
            keepFullRecentRuns: 12,
        },
        detailLimit: 8,
    });

    assert.deepEqual(calls, [
        {
            command: 'get_tauritavern_settings',
            args: undefined,
        },
        {
            command: 'update_tauritavern_settings',
            args: {
                dto: {
                    agent: {
                        retention: {
                            auto_prune_enabled: false,
                            keep_recent_terminal_runs: 80,
                            keep_full_recent_runs: 12,
                        },
                    },
                },
            },
        },
        {
            command: 'plan_agent_run_prune',
            args: {
                dto: {
                    retention: {
                        keepRecentTerminalRuns: 80,
                        keepFullRecentRuns: 12,
                    },
                    detailLimit: 8,
                },
            },
        },
        {
            command: 'apply_agent_run_prune',
            args: {
                dto: {
                    retention: {
                        keepRecentTerminalRuns: 80,
                        keepFullRecentRuns: 12,
                    },
                    detailLimit: 8,
                },
            },
        },
    ]);
});

test('api.agent.retention fails fast on invalid retention inputs', async () => {
    const { calls, agent } = await installHarness();

    await assert.rejects(
        () => agent.retention.updateSettings(null),
        /Agent retention settings update must be an object/,
    );
    await assert.rejects(
        () => agent.retention.updateSettings({}),
        /Agent retention update cannot be empty/,
    );
    await assert.rejects(
        () => agent.retention.updateSettings({ keepRecentTerminalRuns: -1 }),
        /keepRecentTerminalRuns must be an integer between 0 and 10000/,
    );
    await assert.rejects(
        () => agent.retention.updateSettings({ autoPruneEnabled: 'true' }),
        /autoPruneEnabled must be a boolean/,
    );
    await assert.rejects(
        () => agent.retention.updateSettings({ keepRecentTerminalRuns: 10, keepFullRecentRuns: 11 }),
        /keepFullRecentRuns must be less than or equal to keepRecentTerminalRuns/,
    );
    await assert.rejects(
        () => agent.retention.planPrune(null),
        /Agent planRunPrune input must be an object/,
    );
    await assert.rejects(
        () => agent.retention.planPrune({ detailLimit: -1 }),
        /detailLimit must be an integer between 0 and 1000/,
    );
    await assert.rejects(
        () => agent.retention.planPrune({
            retention: {
                keepRecentTerminalRuns: 10,
                keepFullRecentRuns: 11,
            },
        }),
        /keepFullRecentRuns must be less than or equal to keepRecentTerminalRuns/,
    );
    await assert.rejects(
        () => agent.retention.applyPrune(null),
        /Agent applyRunPrune input must be an object/,
    );
    await assert.rejects(
        () => agent.retention.applyPrune({ detailLimit: -1 }),
        /detailLimit must be an integer between 0 and 1000/,
    );
    await assert.rejects(
        () => agent.retention.applyPrune({
            retention: {
                keepRecentTerminalRuns: 10,
                keepFullRecentRuns: 11,
            },
        }),
        /keepFullRecentRuns must be less than or equal to keepRecentTerminalRuns/,
    );
    assert.deepEqual(calls, []);
});

test('api.agent.readModelTurn forwards camelCase DTO and fails fast on invalid input', async () => {
    const { calls, agent } = await installHarness();

    await agent.readModelTurn({ runId: 'run-1', invocationId: 'inv_child', round: 2, maxChars: 12000 });
    await agent.readModelTurn({ runId: 'run-1', round: 3 });

    assert.deepEqual(calls, [
        {
            command: 'read_agent_model_turn',
            args: { dto: { runId: 'run-1', invocationId: 'inv_child', round: 2, maxChars: 12000 } },
        },
        {
            command: 'read_agent_model_turn',
            args: { dto: { runId: 'run-1', round: 3 } },
        },
    ]);

    await assert.rejects(
        () => agent.readModelTurn({ runId: '', round: 1 }),
        /runId is required/,
    );
    await assert.rejects(
        () => agent.readModelTurn({ runId: 'run-1', round: 0 }),
        /round must be a positive integer/,
    );
    await assert.rejects(
        () => agent.readModelTurn({ runId: 'run-1', round: 1, maxChars: 0 }),
        /maxChars must be a positive integer/,
    );
});

test('api.agent.pruneChatPersistentStates forwards explicit candidate state ids', async () => {
    const { calls, agent } = await installHarness();
    const chatRef = { kind: 'character', characterId: 'char-1', fileName: 'Char.json' };

    await agent.pruneChatPersistentStates({
        chatRef,
        stableChatId: ' chat_1 ',
        candidateStateIds: [' state_drop ', 'state_drop', 'state_keep'],
    });

    assert.deepEqual(calls, [
        {
            command: 'prune_agent_chat_persistent_states',
            args: {
                dto: {
                    chatRef,
                    stableChatId: 'chat_1',
                    candidateStateIds: ['state_drop', 'state_keep'],
                },
            },
        },
    ]);
});

test('api.agent.pruneChatPersistentStates fails fast on invalid candidate state ids', async () => {
    const { calls, agent } = await installHarness();
    const input = {
        chatRef: { kind: 'character', characterId: 'char-1', fileName: 'Char.json' },
        stableChatId: 'chat_1',
    };

    await assert.rejects(
        () => agent.pruneChatPersistentStates(input),
        /candidateStateIds must be an array/,
    );
    await assert.rejects(
        () => agent.pruneChatPersistentStates({ ...input, candidateStateIds: 'state_drop' }),
        /candidateStateIds must be an array/,
    );
    await assert.rejects(
        () => agent.pruneChatPersistentStates({ ...input, candidateStateIds: ['state_drop', ''] }),
        /candidateStateIds contains an empty state id/,
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

test('agent prompt assembly bridge reads pending request by assembly id', async () => {
    const moduleUrl = pathToFileURL(path.join(REPO_ROOT, 'src/tauri/main/api/agent-prompt-assembly-bridge.js'));
    moduleUrl.search = `?case=prompt-assembly-request-read-${Date.now()}`;
    const { attachHostPromptAssemblyBridge } = await import(moduleUrl.href);
    const calls = [];
    let listener = null;
    let seenRequest = null;

    attachHostPromptAssemblyBridge({
        runId: 'run-prompt-assembly',
        safeInvoke: async (command, args) => {
            calls.push({ command, args });
            if (command === 'read_agent_prompt_assembly_request') {
                return {
                    kind: 'tauritavern.agentPromptAssemblyRequest',
                    schemaVersion: 1,
                    frozenRunInputSnapshot: { promptInputs: {}, worldInfoActivation: {}, macroContext: {} },
                    settings: { chat_completion_source: 'openai', openai_model: 'test-model' },
                };
            }
            return {};
        },
        promptAssembly: {
            async buildSnapshot(request) {
                seenRequest = request;
                return {
                    promptSnapshot: {
                        contextPolicy: {},
                        chatCompletionPayload: { messages: [{ role: 'user', content: 'assembled' }] },
                    },
                    frozenRunInputSnapshot: request.frozenRunInputSnapshot,
                    generationIntent: { source: 'test' },
                    assembly: { engine: 'test' },
                };
            },
        },
        subscribe(runId, handler) {
            assert.equal(runId, 'run-prompt-assembly');
            listener = handler;
            return () => {};
        },
    });

    listener({
        type: 'prompt_assembly_requested',
        payload: {
            assemblyId: 'prompt_assembly_1',
            requestKind: 'tauritavern.agentPromptAssemblyRequest',
        },
    });

    await waitFor(() => calls.some(call => call.command === 'resolve_agent_prompt_assembly'));

    assert.equal(seenRequest.kind, 'tauritavern.agentPromptAssemblyRequest');
    assert.deepEqual(calls.map(call => call.command), [
        'read_agent_prompt_assembly_request',
        'resolve_agent_prompt_assembly',
    ]);
    assert.deepEqual(calls[0].args, {
        dto: {
            runId: 'run-prompt-assembly',
            assemblyId: 'prompt_assembly_1',
        },
    });
    assert.equal(calls[1].args.dto.assemblyId, 'prompt_assembly_1');
    assert.equal(calls[1].args.dto.promptSnapshot.chatCompletionPayload.messages[0].content, 'assembled');
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
