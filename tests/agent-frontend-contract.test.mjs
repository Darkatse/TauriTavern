import test from 'node:test';
import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
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
        selectedProfileId: 'default-writer',
        activeTab: 'profiles',
    });
    assert.equal(writes.length, 0);

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

test('Agent generation router uses the global toggle for normal regenerate and swipe', async () => {
    let stored = {
        agentModeEnabled: false,
        selectedProfileId: 'default-writer',
        activeTab: 'profiles',
    };
    installWindow({
        extension: {
            store: {
                async tryGetJson() {
                    return { found: true, value: stored };
                },
                async setJson(request) {
                    stored = request.value;
                },
            },
        },
    });

    const router = await importFresh('src/scripts/tauritavern/agent/agent-generation-router.js');

    assert.deepEqual(await router.getAgentGenerationOptions({
        generationType: 'normal',
        mainApi: 'openai',
    }), {});

    stored = {
        ...stored,
        agentModeEnabled: true,
        selectedProfileId: 'writer',
    };

    for (const generationType of ['normal', 'regenerate', 'swipe']) {
        assert.deepEqual(await router.getAgentGenerationOptions({
            generationType,
            mainApi: 'openai',
        }), {
            agentMode: true,
            agentProfileId: 'writer',
        });
    }

    assert.deepEqual(await router.getAgentGenerationOptions({
        generationType: 'normal',
        isSlashCommand: true,
        mainApi: 'openai',
    }), {});

    await assert.rejects(
        () => router.getAgentGenerationOptions({ generationType: 'continue', mainApi: 'openai' }),
        /agent\.generation_type_unsupported/,
    );
    await assert.rejects(
        () => router.getAgentGenerationOptions({ generationType: 'normal', mainApi: 'kobold' }),
        /agent\.chat_completion_required/,
    );
    await assert.rejects(
        () => router.getAgentGenerationOptions({ generationType: 'normal', mainApi: 'openai', selectedGroup: 'group-1' }),
        /agent\.group_chat_unsupported/,
    );
});

test('Agent System confirmations use SillyTavern Popup instead of window.confirm', async () => {
    const calls = [];
    installWindow({});
    globalThis.window.confirm = () => {
        throw new Error('window.confirm must not be used');
    };
    globalThis.window.SillyTavern = {
        getContext() {
            return {
                POPUP_RESULT: { AFFIRMATIVE: 1 },
                Popup: {
                    show: {
                        async confirm(header, message) {
                            calls.push({ header, message });
                            return 1;
                        },
                    },
                },
            };
        },
    };

    const { confirmAction } = await importFresh('src/scripts/extensions/agent-system/src/host-api.js');

    assert.equal(await confirmAction('Delete Skill "test-skill"?'), true);
    assert.deepEqual(calls, [{ header: null, message: 'Delete Skill "test-skill"?' }]);
});

test('Agent System CSS does not globally override upstream utility classes', async () => {
    const css = await readFile(path.join(
        REPO_ROOT,
        'src/scripts/extensions/agent-system/style.css',
    ), 'utf8');
    const leakedSelectors = [];
    const rulePattern = /([^{}]+)\{/g;
    let match;

    while ((match = rulePattern.exec(css)) !== null) {
        const selectorGroup = match[1].trim();
        if (!selectorGroup || selectorGroup.startsWith('@')) {
            continue;
        }

        for (const rawSelector of selectorGroup.split(',')) {
            const selector = rawSelector.trim();
            const scopedToAgent = selector.includes('.ttas-') || selector.includes('#agent_system_settings');
            const touchesUpstreamUtility = /(?:^|[\s>+~])\.(?:textarea_compact|text_pole|menu_button)\b/.test(selector);
            if (!scopedToAgent && touchesUpstreamUtility) {
                leakedSelectors.push(selector);
            }
        }
    }

    assert.deepEqual(leakedSelectors, []);
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

test('PromptManager uses Agent System Prompt as a runtime-resolved marker', async () => {
    const promptManagerSource = await readFile(path.join(REPO_ROOT, 'src/scripts/PromptManager.js'), 'utf8');
    const openAiSource = await readFile(path.join(REPO_ROOT, 'src/scripts/openai.js'), 'utf8');

    assert.match(promptManagerSource, /const AGENT_SYSTEM_PROMPT_IDENTIFIER = 'agentSystemPrompt';/);
    assert.match(promptManagerSource, /normalizeAgentSystemPromptDefinition\(\)/);
    assert.match(promptManagerSource, /marker:\s*true/);
    assert.doesNotMatch(promptManagerSource, /case 'agentSystemPrompt':/);

    assert.match(openAiSource, /_tauritavern_agent_prompt_marker/);
    assert.match(openAiSource, /populateAgentSystemPromptMarker/);
    assert.doesNotMatch(openAiSource, /\[AGENT_SYSTEM_PROMPT_IDENTIFIER,\s*'nsfw'/);
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

test('Agent run event presenter keeps timeline projection focused', async () => {
    const presenter = await importFresh('src/scripts/extensions/agent-system/src/run-event-presenter.js');

    const debugEvent = {
        seq: 1,
        id: 'evt-debug',
        runId: 'run-1',
        type: 'tool_result_stored',
        payload: { callId: 'call-1', path: 'tool-results/call-1.json' },
    };
    const toolEvent = {
        seq: 2,
        id: 'evt-tool',
        runId: 'run-1',
        type: 'tool_call_requested',
        timestamp: '2026-05-04T12:00:00Z',
        level: 'info',
        payload: {
            callId: 'call-1',
            name: 'workspace.write_file',
            argumentsRef: 'tool-args/call-1.json',
        },
    };

    assert.equal(presenter.isDisplayableRunEvent(debugEvent), false);
    assert.equal(presenter.isDisplayableRunEvent(toolEvent), true);

    const item = presenter.presentRunEvent(toolEvent);
    assert.equal(item.titleKey, 'timelineEventToolRequested');
    assert.deepEqual(item.titleParams, { tool: 'writing a file' });
    assert.equal(item.summary, 'call-1');

    const projected = presenter.timelineItemsFromEvents([
        debugEvent,
        toolEvent,
        {
            seq: 3,
            id: 'evt-completed',
            runId: 'run-1',
            type: 'tool_call_completed',
            payload: { callId: 'call-1', name: 'workspace.write_file' },
        },
        {
            seq: 4,
            id: 'evt-write',
            runId: 'run-1',
            type: 'workspace_file_written',
            payload: { path: 'output/main.md', bytes: 12 },
        },
    ]);
    assert.deepEqual(projected.map(event => event.type), ['workspace_file_written']);
});

test('Agent run tool labels stay user-facing in timeline projection', async () => {
    const { displayToolName } = await importFresh('src/scripts/extensions/agent-system/src/run-tool-labels.js');

    assert.equal(displayToolName('skill.read'), 'reading a skill');
    assert.equal(displayToolName('workspace.write_file'), 'writing a file');
    assert.equal(displayToolName('vendor.custom_action'), 'custom action');
});

test('Agent run event presenter derives lazy detail targets from journal refs', async () => {
    const presenter = await importFresh('src/scripts/extensions/agent-system/src/run-event-presenter.js');
    const resultEvent = {
        seq: 1,
        id: 'evt-result',
        runId: 'run-1',
        type: 'tool_result_stored',
        payload: { callId: 'call-1', path: 'tool-results/call-1.json' },
    };
    const completed = {
        seq: 2,
        id: 'evt-completed',
        runId: 'run-1',
        type: 'tool_call_completed',
        payload: {
            callId: 'call-1',
            name: 'workspace.write_file',
            resourceRefs: ['output/main.md'],
        },
    };

    const targets = presenter.buildEventDetailTargets(
        presenter.presentRunEvent(completed),
        [resultEvent, completed],
    );

    assert.deepEqual(targets.map(target => [target.type, target.labelKey, target.path || '']), [
        ['file', 'timelineToolResult', 'tool-results/call-1.json'],
    ]);

    const writeEvent = {
        seq: 3,
        id: 'evt-write',
        runId: 'run-1',
        type: 'workspace_file_written',
        payload: { path: 'output/main.md', bytes: 12 },
    };
    const writeTargets = presenter.buildEventDetailTargets(
        presenter.presentRunEvent(writeEvent),
        [resultEvent, completed, writeEvent],
    );

    assert.deepEqual(writeTargets.map(target => [target.type, target.labelKey, target.path || '']), [
        ['file', 'timelineWorkspaceFile', 'output/main.md'],
    ]);
});

test('Agent run event presenter keeps model turns out of timeline and exposes reasoning lazily', async () => {
    const presenter = await importFresh('src/scripts/extensions/agent-system/src/run-event-presenter.js');
    const modelEvent = {
        seq: 4,
        id: 'evt-model',
        runId: 'run-1',
        type: 'model_completed',
        timestamp: '2026-05-04T12:00:00Z',
        level: 'info',
        payload: {
            round: 2,
            modelResponsePath: 'model-responses/round-002.json',
            toolCallCount: 1,
            hasReasoning: true,
            reasoningBytes: 30,
        },
    };
    const toolEvent = {
        seq: 5,
        id: 'evt-tool',
        runId: 'run-1',
        type: 'tool_call_completed',
        payload: {
            round: 2,
            callId: 'call-1',
            name: 'workspace.read_file',
        },
    };

    assert.equal(presenter.isDisplayableRunEvent(modelEvent), false);
    assert.deepEqual(presenter.timelineItemsFromEvents([modelEvent]).map(item => item.type), []);

    const targets = presenter.buildEventDetailTargets(
        presenter.presentRunEvent(toolEvent),
        [modelEvent, toolEvent],
    );
    assert.deepEqual(targets, [
        { type: 'modelReasoning', labelKey: 'timelineReasoning', round: 2 },
    ]);
});

test('Agent run detail formatter renders tool result details without raw JSON shell', async () => {
    const { formatDetailFile } = await importFresh('src/scripts/extensions/agent-system/src/run-detail-format.js');
    const section = formatDetailFile(
        { labelKey: 'timelineToolResult', path: 'tool-results/call-1.json' },
        {
            path: 'tool-results/call-1.json',
            bytes: 248,
            sha256: '0123456789abcdef0123456789abcdef',
            text: JSON.stringify({
                callId: 'call-1',
                name: 'workspace.read_file',
                content: 'output/main.md lines 1-2 of 2, sha256 abc\n1 hello\n2 world',
                structured: {
                    path: 'output/main.md',
                    totalLines: 2,
                    startLine: 1,
                    endLine: 2,
                    fullRead: true,
                },
                isError: false,
                resourceRefs: ['output/main.md'],
            }, null, 2),
        },
    );

    assert.equal(section.labelKey, 'timelineToolResult');
    assert.equal(section.blocks[0].labelKey, 'timelineResultText');
    assert.match(section.blocks[0].text, /1 hello/);
    assert.doesNotMatch(section.blocks[0].text, /sha256/);
    assert.doesNotMatch(section.blocks[0].text, /"callId"/);
    assert.deepEqual(section.fields, [
        { label: 'Operation', value: 'reading a file' },
        { label: 'Target', value: 'output/main.md' },
        { label: 'Range', value: 'full file' },
    ]);
});

test('Agent run detail formatter renders model turn display DTO', async () => {
    const { formatModelTurnDetail } = await importFresh('src/scripts/extensions/agent-system/src/run-detail-format.js');
    const section = formatModelTurnDetail(
        { type: 'modelReasoning', labelKey: 'timelineReasoning', round: 2 },
        {
            runId: 'run-1',
            round: 2,
            modelResponsePath: 'model-responses/round-002.json',
            provider: {
                source: 'openai',
                format: 'responses',
                model: 'gpt-5',
                responseId: 'resp_1',
            },
            assistant: {
                text: 'I will inspect the workspace.',
                bytes: 29,
                truncated: false,
            },
            reasoning: [{
                source: 'reasoning_content',
                text: 'Need to inspect the workspace.',
                bytes: 30,
                truncated: true,
            }],
            toolCalls: [{
                callId: 'call-1',
                name: 'workspace.read_file',
            }],
        },
    );

    assert.equal(section.labelKey, 'timelineReasoning');
    assert.equal(section.path, '');
    assert.deepEqual(section.fields, [
        { label: 'Round', value: '2' },
        { label: 'Provider', value: 'openai / responses' },
        { label: 'Model', value: 'gpt-5' },
    ]);
    assert.deepEqual(section.blocks.map(block => block.labelKey), [
        'timelineReasoning',
    ]);
    assert.equal(section.blocks[0].truncated, true);
    assert.equal(section.blocks[0].kind, 'reasoning');
    assert.equal(section.blocks[0].defaultOpen, false);
    assert.equal(section.blocks[0].meta, 'reasoning_content · 30 bytes');
    assert.match(section.blocks[0].text, /Need to inspect/);
});
