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

    const { installAgentApi } = await import(pathToFileURL(path.join(REPO_ROOT, 'src/tauri/main/api/agent.js')));
    installAgentApi({
        safeInvoke: async (command, args) => {
            calls.push({ command, args });
            return { command, args };
        },
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
    await agent.profiles.resolveSystemPrompt({ profileId: 'writer' });
    await agent.profiles.save({ profile });
    await agent.profiles.delete('writer');

    assert.deepEqual(calls, [
        { command: 'list_agent_profiles', args: undefined },
        { command: 'load_agent_profile', args: { dto: { profileId: 'writer' } } },
        { command: 'resolve_agent_system_prompt', args: { dto: { profileId: 'writer' } } },
        { command: 'save_agent_profile', args: { dto: { profile } } },
        { command: 'delete_agent_profile', args: { dto: { profileId: 'writer' } } },
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
    await agent.profiles.delete('writer');
    unsubscribe();

    assert.deepEqual(events, ['changed', 'changed']);
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

test('api.agent.readModelTurn forwards camelCase DTO and fails fast on invalid input', async () => {
    const { calls, agent } = await installHarness();

    await agent.readModelTurn({ runId: 'run-1', round: 2, maxChars: 12000 });
    await agent.readModelTurn({ runId: 'run-1', round: 3 });

    assert.deepEqual(calls, [
        {
            command: 'read_agent_model_turn',
            args: { dto: { runId: 'run-1', round: 2, maxChars: 12000 } },
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
