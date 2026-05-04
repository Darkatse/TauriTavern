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

test('Agent System settings use the extension store and publish changes', async () => {
    const writes = [];
    let stored = null;
    installWindow({
        extension: {
            store: {
                async getJson() {
                    if (stored === null) {
                        throw new Error('not found');
                    }
                    return stored;
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
        selectedProfileId: 'default-writer',
        activeTab: 'profiles',
    });
    assert.equal(writes.length, 1);

    let emitted = null;
    const unsubscribe = settings.subscribeAgentSystemSettings((next) => {
        emitted = next;
    });
    const saved = await settings.saveAgentSystemSettings({
        agentModeEnabled: true,
        selectedProfileId: 'writer',
    });
    unsubscribe();

    assert.deepEqual(saved, {
        agentModeEnabled: true,
        selectedProfileId: 'writer',
        activeTab: 'profiles',
    });
    assert.deepEqual(emitted, saved);
});

test('default Agent profile exposes the effective default system prompt in frontend drafts', async () => {
    const {
        DEFAULT_PROFILE_ID,
    } = await importFresh('src/scripts/extensions/agent-system/src/constants.js');
    const {
        buildDefaultAgentSystemPrompt,
        defaultProfile,
        profileForEdit,
    } = await importFresh('src/scripts/extensions/agent-system/src/profile-model.js');

    const profile = defaultProfile();
    assert.equal(profile.id, DEFAULT_PROFILE_ID);
    assert.equal(profile.instructions.agentSystemPrompt, buildDefaultAgentSystemPrompt(profile));
    assert.match(profile.instructions.agentSystemPrompt, /TauriTavern Agent Mode is active\./);
    assert.match(profile.instructions.agentSystemPrompt, /workspace_commit/);
    assert.match(profile.instructions.agentSystemPrompt, /workspace_finish/);

    const backendBuiltIn = {
        ...profile,
        instructions: { agentSystemPrompt: null },
    };
    const draft = profileForEdit(backendBuiltIn);
    assert.equal(draft.instructions.agentSystemPrompt, buildDefaultAgentSystemPrompt(draft));
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
