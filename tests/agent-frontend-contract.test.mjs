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

async function createAgentPanelHarness() {
    const { createAgentSystemPanelRoot } = await importFresh('src/scripts/extensions/agent-system/src/AgentSystemPanelApp.js');
    const options = createAgentSystemPanelRoot({ requestClose() {} });
    const vm = options.data();
    for (const [name, method] of Object.entries(options.methods || {})) {
        vm[name] = method.bind(vm);
    }
    for (const [name, computed] of Object.entries(options.computed || {})) {
        Object.defineProperty(vm, name, {
            configurable: true,
            enumerable: true,
            get: computed.bind(vm),
        });
    }
    vm.$el = { querySelector: () => null };
    vm.$nextTick = (callback) => callback();
    return vm;
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
        activeProfileId: 'writer',
        editingProfileId: 'editor',
    });
    unsubscribe();

    assert.deepEqual(saved, {
        agentModeEnabled: true,
        activeProfileId: 'writer',
        editingProfileId: 'editor',
        activeTab: 'profiles',
        runTimelineHeightPx: null,
    });
    assert.deepEqual(emitted, saved);
});

test('Agent run timeline resize geometry is deterministic', async () => {
    const resize = await importFresh('src/scripts/extensions/agent-system/src/run-timeline-resize.js');

    assert.equal(resize.normalizeRunTimelineHeightPx(null), null);
    assert.equal(resize.normalizeRunTimelineHeightPx(420.4), 420);
    assert.throws(() => resize.normalizeRunTimelineHeightPx('420'), /finite number or null/);

    const bounds = resize.runTimelineHeightBounds({
        panelBottom: 700,
        topBoundary: 100,
        chromeHeight: 40,
    });
    assert.deepEqual(bounds, { min: 132, max: 548 });

    assert.equal(resize.clampRunTimelineHeightPx(80, bounds), 132);
    assert.equal(resize.clampRunTimelineHeightPx(900, bounds), 548);
    assert.equal(resize.heightFromTopEdgeDrag({
        startHeight: 300,
        startY: 500,
        currentY: 420,
        bounds,
    }), 380);
});

test('Agent run timeline projects SubAgent tasks without flattening child events into root', async () => {
    const projector = await importFresh('src/scripts/extensions/agent-system/src/run-invocation-projector.js');
    const presenter = await importFresh('src/scripts/extensions/agent-system/src/run-event-presenter.js');
    const events = [
        {
            seq: 1,
            id: 'evt-root-tool',
            runId: 'run-1',
            type: 'tool_call_completed',
            payload: { invocationId: 'inv_root', callId: 'call_delegate', name: 'agent.delegate' },
        },
        {
            seq: 2,
            id: 'evt-delegate',
            runId: 'run-1',
            type: 'agent_delegate_started',
            payload: {
                taskId: 'task-1',
                parentInvocationId: 'inv_root',
                childInvocationId: 'inv-child',
                targetProfileId: 'scene-critic',
                workspaceKey: 'scene-critic',
            },
        },
        {
            seq: 3,
            id: 'evt-task-start',
            runId: 'run-1',
            type: 'agent_task_started',
            payload: {
                taskId: 'task-1',
                childInvocationId: 'inv-child',
                targetProfileId: 'scene-critic',
                status: 'running',
            },
        },
        {
            seq: 4,
            id: 'evt-child-model',
            runId: 'run-1',
            type: 'model_completed',
            payload: {
                invocationId: 'inv-child',
                round: 1,
                toolCallCount: 1,
                hasReasoning: true,
                reasoningChars: 12,
                reasoningWords: 2,
            },
        },
        {
            seq: 5,
            id: 'evt-child-tool',
            runId: 'run-1',
            type: 'tool_call_completed',
            payload: { invocationId: 'inv-child', callId: 'call_return', name: 'task.return' },
        },
        {
            seq: 6,
            id: 'evt-return',
            runId: 'run-1',
            type: 'task_return_completed',
            payload: {
                taskId: 'task-1',
                childInvocationId: 'inv-child',
                status: 'completed',
                resultRef: 'agent-results/inv-child.json',
                summaryRef: 'summaries/agents/scene-critic/result.md',
            },
        },
    ];

    const projection = projector.projectAgentInvocations(events);
    assert.equal(projection.subAgentTasks.length, 1);
    assert.equal(projection.subAgentTasks[0].displayName, 'scene-critic');
    assert.equal(projection.subAgentTasks[0].status, 'completed');

    const rootItems = presenter.timelineItemsFromEvents(events, { invocationId: projector.ROOT_INVOCATION_ID });
    assert.deepEqual(rootItems.map(item => item.type), ['agent_delegate_started']);

    const childItems = presenter.timelineItemsFromEvents(
        projector.eventsForInvocation(events, 'inv-child'),
        { invocationId: 'inv-child' },
    );
    assert.deepEqual(childItems.map(item => item.type), [
        'agent_delegate_started',
        'agent_task_started',
        'task_return_completed',
    ]);
});

test('Agent generation router uses the global toggle for normal regenerate and swipe', async () => {
    let stored = {
        agentModeEnabled: false,
        activeProfileId: 'default-writer',
        editingProfileId: 'default-writer',
        activeTab: 'profiles',
        runTimelineHeightPx: null,
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
        agent: {
            profiles: {
                async load({ profileId }) {
                    assert.equal(profileId, stored.activeProfileId);
                    return {
                        profile: {
                            context: {
                                initialChatHistoryMessages: 6,
                                includeActivatedWorldInfo: false,
                            },
                        },
                    };
                },
                async resolveSystemPrompt({ profileId } = {}) {
                    assert.equal(profileId, stored.activeProfileId);
                    return { agentSystemPrompt: 'Resolved Agent System Prompt.' };
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
        activeProfileId: 'writer',
    };

    for (const generationType of ['normal', 'regenerate', 'swipe']) {
        assert.deepEqual(await router.getAgentGenerationOptions({
            generationType,
            mainApi: 'openai',
        }), {
            agentMode: true,
            agentProfileId: 'writer',
            agentContextPolicy: {
                initialChatHistoryMessages: 6,
                includeActivatedWorldInfo: false,
            },
            agentSystemPrompt: 'Resolved Agent System Prompt.',
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

test('FrozenRunInputSnapshot stores materialized extension prompts and macro context', async () => {
    const frozen = await importFresh('src/scripts/tauritavern/agent/frozen-run-input-snapshot.js');
    const extensionPrompts = await frozen.snapshotExtensionPromptsForFrozenRun({
        active: {
            value: 'Visible prompt',
            position: 1,
            depth: 2,
            scan: true,
            role: 0,
            filter: () => true,
        },
        inactive: {
            value: 'Hidden prompt',
            position: 1,
            depth: 2,
            scan: true,
            role: 0,
            filter: async () => false,
        },
    });

    assert.deepEqual(extensionPrompts, {
        active: {
            value: 'Visible prompt',
            position: 1,
            depth: 2,
            scan: true,
            role: 0,
        },
    });

    const snapshot = frozen.buildFrozenRunInputSnapshot({
        generationType: 'swipe',
        promptInputs: { type: 'swipe', extensionPrompts },
        worldInfoActivation: { entries: [] },
        macroContext: { names: { user: 'User', char: 'Char' } },
    });
    const normalized = frozen.normalizeFrozenRunInputSnapshot(snapshot);

    assert.equal(normalized.generationType, 'swipe');
    assert.equal(normalized.macroContext.names.char, 'Char');
    assert.equal(Object.hasOwn(normalized.promptInputs.extensionPrompts.active, 'filter'), false);
});

test('/trigger routes Agent generation fail-fast without Legacy fallback', async () => {
    const source = await readFile(path.join(REPO_ROOT, 'src/scripts/slash-commands.js'), 'utf8');
    const start = source.indexOf('async function triggerGenerationCallback');
    const end = source.indexOf('/**\n * Find persona by name.', start);
    assert.ok(start >= 0 && end > start, 'triggerGenerationCallback section must be present');

    const section = source.slice(start, end);
    assert.match(section, /runTriggeredGeneration/);
    assert.match(section, /getAgentGenerationOptions\(\{\s*generationType: 'normal',\s*isSlashCommand: false,\s*mainApi: main_api,\s*selectedGroup: selected_group,\s*\}\)/s);
    assert.match(section, /toastr\.error\(agentErrorMessage\(error\), t`Agent Mode`\)/);
    assert.match(section, /return Generate\('normal', \{ force_chid: chid, \.\.\.agentOptions \}\)/);
    assert.doesNotMatch(section, /\.catch\(\(\) => \(\{\}\)\)/);

    const routeCall = section.slice(section.indexOf('getAgentGenerationOptions'));
    assert.doesNotMatch(routeCall, /getAgentGenerationOptions[\s\S]*?\.catch\s*\(/);
});

test('/regenerate routes Agent generation fail-fast without Legacy fallback', async () => {
    const source = await readFile(path.join(REPO_ROOT, 'src/scripts/slash-commands.js'), 'utf8');
    const start = source.indexOf('async function regenerateChatCallback');
    const end = source.indexOf('async function swipeChatCallback', start);
    assert.ok(start >= 0 && end > start, 'regenerateChatCallback section must be present');

    const section = source.slice(start, end);
    assert.match(section, /runRegeneration/);
    assert.match(section, /getAgentGenerationOptions\(\{\s*generationType: 'regenerate',\s*mainApi: main_api,\s*selectedGroup: selected_group,\s*\}\)/s);
    assert.match(section, /toastr\.error\(agentErrorMessage\(error\), t`Agent Mode`\)/);
    assert.match(section, /return Generate\('regenerate', agentOptions\)/);
    assert.doesNotMatch(section, /getAgentGenerationOptions[\s\S]*?\.catch\s*\(/);
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

test('Agent profile drafts keep Agent system prompt owned by the backend resolver', async () => {
    const {
        DEFAULT_PROFILE_ID,
    } = await importFresh('src/scripts/extensions/agent-system/src/constants.js');
    const {
        defaultProfile,
        normalizeProfileForSave,
        profileForEdit,
    } = await importFresh('src/scripts/extensions/agent-system/src/profile-model.js');
    const profileModelSource = await readFile(path.join(REPO_ROOT, 'src/scripts/extensions/agent-system/src/profile-model.js'), 'utf8');
    const panelSource = await readFile(path.join(REPO_ROOT, 'src/scripts/extensions/agent-system/src/AgentSystemPanelApp.js'), 'utf8');

    const profile = defaultProfile();
    assert.equal(profile.id, DEFAULT_PROFILE_ID);
    assert.equal(profile.instructions.agentSystemPrompt, null);

    const backendBuiltIn = {
        ...profile,
        instructions: { agentSystemPrompt: null },
    };
    const draft = profileForEdit(backendBuiltIn);
    assert.equal(draft.instructions.agentSystemPrompt, null);
    assert.equal(normalizeProfileForSave(draft).instructions.agentSystemPrompt, null);
    assert.doesNotMatch(profileModelSource, /buildDefaultAgentSystemPrompt/);
    assert.match(panelSource, /resolveSystemPrompt/);
    assert.match(panelSource, /resolvedAgentSystemPrompt/);
});

test('Agent profile save normalization keeps delegation tools contract-shaped', async () => {
    const {
        defaultProfile,
        normalizeProfileForSave,
        profileForEdit,
    } = await importFresh('src/scripts/extensions/agent-system/src/profile-model.js');

    const draft = profileForEdit(defaultProfile('delegate-writer'));
    draft.delegation.canDelegate = true;
    draft.tools.allow.push('task.return');

    const saved = normalizeProfileForSave(draft);
    assert.equal(saved.delegation.canDelegate, true);
    assert(saved.tools.allow.includes('agent.list'));
    assert(saved.tools.allow.includes('agent.delegate'));
    assert(saved.tools.allow.includes('agent.await'));
    assert(!saved.tools.allow.includes('task.return'));

    saved.delegation.canDelegate = false;
    const disabled = normalizeProfileForSave(profileForEdit(saved));
    assert(!disabled.tools.allow.includes('agent.list'));
    assert(!disabled.tools.allow.includes('agent.delegate'));
    assert(!disabled.tools.allow.includes('agent.await'));
});

test('Agent profile callable SubAgent toggle owns non-direct run semantics', async () => {
    const vm = await createAgentPanelHarness();
    vm.draft.id = 'scene-consultant';
    vm.draft.run.presentation = 'foreground';
    vm.seedMainAgentPresentation();

    vm.setProfileEditMode('subagent');
    assert.equal(vm.draft.run.directRunnable, true);
    assert.equal(vm.draft.run.presentation, 'foreground');

    vm.setCallableAsSubAgent(true);

    assert.equal(vm.draft.delegation.callable, true);
    assert.equal(vm.draft.delegation.allowAsSubagent, true);
    assert.equal(vm.draft.run.directRunnable, false);
    assert.equal(vm.draft.run.presentation, 'background');
    assert.equal(vm.isSubAgentPresentationLocked, true);
    assert.throws(
        () => vm.setRunPresentation('foreground'),
        /SubAgent-only profiles are locked/,
    );

    vm.setCallableAsSubAgent(false);
    assert.equal(vm.profileEditMode, 'main');
    assert.equal(vm.draft.run.directRunnable, true);
    assert.equal(vm.draft.run.presentation, 'foreground');
});

test('Agent profile edit mode follows loaded profile without mutating run policy', async () => {
    const {
        defaultProfile,
    } = await importFresh('src/scripts/extensions/agent-system/src/profile-model.js');
    const directMain = defaultProfile('direct-writer');
    directMain.run.presentation = 'foreground';

    const callable = defaultProfile('callable-consultant');
    callable.run.presentation = 'foreground';
    callable.delegation.callable = true;
    callable.delegation.allowAsSubagent = true;

    const backgroundOnly = defaultProfile('background-consultant');
    backgroundOnly.run.presentation = 'background';
    backgroundOnly.run.directRunnable = false;
    backgroundOnly.delegation.callable = true;
    backgroundOnly.delegation.allowAsSubagent = true;

    const profiles = new Map([
        [directMain.id, directMain],
        [callable.id, callable],
        [backgroundOnly.id, backgroundOnly],
    ]);
    let settings = null;
    installWindow({
        extension: {
            store: {
                async setJson(request) {
                    settings = request.value;
                },
            },
        },
        agent: {
            profiles: {
                async load({ profileId }) {
                    return { profile: profiles.get(profileId) };
                },
                async resolveSystemPrompt() {
                    return { agentSystemPrompt: 'Resolved Agent system prompt.' };
                },
            },
        },
    });
    globalThis.toastr = {
        success() {},
        warning() {},
        error(error) {
            throw new Error(String(error || 'unexpected toastr error'));
        },
    };

    const vm = await createAgentPanelHarness();
    await vm.selectProfile(directMain.id);
    assert.equal(vm.profileEditMode, 'main');
    vm.setProfileEditMode('subagent');
    assert.equal(vm.draft.run.directRunnable, true);
    assert.equal(vm.draft.run.presentation, 'foreground');
    vm.setProfileEditMode('main');
    assert.equal(vm.draft.run.presentation, 'foreground');

    vm.setProfileEditMode('subagent');
    await vm.selectProfile(callable.id);
    assert.equal(vm.profileEditMode, 'main');
    assert.equal(vm.draft.run.directRunnable, true);
    assert.equal(vm.draft.run.presentation, 'foreground');
    vm.setProfileEditMode('subagent');
    assert.equal(vm.draft.run.directRunnable, true);
    assert.equal(vm.draft.run.presentation, 'foreground');

    vm.setProfileEditMode('main');
    await vm.selectProfile(backgroundOnly.id);
    assert.equal(vm.profileEditMode, 'subagent');
    assert.equal(vm.draft.run.directRunnable, false);
    assert.equal(vm.draft.run.presentation, 'background');
    assert.equal(settings.editingProfileId, backgroundOnly.id);
    vm.setProfileEditMode('main');
    assert.equal(vm.draft.run.presentation, 'background');
});

test('Agent profile save keeps non-direct callable profiles out of direct default selection', async () => {
    const {
        defaultProfile,
        profileForEdit,
    } = await importFresh('src/scripts/extensions/agent-system/src/profile-model.js');
    const savedProfiles = new Map();
    let settings = {
        agentModeEnabled: true,
        activeProfileId: 'subagent-only',
        editingProfileId: 'subagent-only',
        activeTab: 'profiles',
        runTimelineHeightPx: null,
    };
    installWindow({
        extension: {
            store: {
                async setJson(request) {
                    settings = request.value;
                },
            },
        },
        agent: {
            profiles: {
                async save({ profile }) {
                    savedProfiles.set(profile.id, profile);
                },
                async list() {
                    return {
                        profiles: [...savedProfiles.values()].map((profile) => ({
                            id: profile.id,
                            displayName: profile.displayName,
                            description: profile.description,
                            directRunnable: profile.run.directRunnable,
                        })),
                    };
                },
                async load({ profileId }) {
                    return { profile: savedProfiles.get(profileId) };
                },
                async resolveSystemPrompt() {
                    return { agentSystemPrompt: 'Resolved Agent system prompt.' };
                },
            },
        },
    });
    globalThis.toastr = {
        success() {},
        warning() {},
        error(error) {
            throw new Error(String(error || 'unexpected toastr error'));
        },
    };

    const vm = await createAgentPanelHarness();
    vm.settings = settings;
    const profile = defaultProfile('subagent-only');
    profile.tools.allow = profile.tools.allow.filter((tool) => tool !== 'workspace.finish');
    vm.editingProfileId = profile.id;
    vm.draft = profileForEdit(profile);
    vm.setProfileEditMode('subagent');
    vm.setCallableAsSubAgent(true);

    await vm.saveProfile();

    assert.equal(savedProfiles.get(profile.id).run.directRunnable, false);
    assert.equal(vm.editingProfileId, profile.id);
    assert.equal(settings.activeProfileId, 'default-writer');
    assert.equal(settings.editingProfileId, profile.id);
});

test('Agent System profile panel no longer owns legacy Skill management UI', async () => {
    const panelSource = await readFile(path.join(
        REPO_ROOT,
        'src/scripts/extensions/agent-system/src/AgentSystemPanelApp.js',
    ), 'utf8');
    const skillExtensionSource = await readFile(path.join(
        REPO_ROOT,
        'src/scripts/extensions/agent-system/src/skill-manager/panel-app.js',
    ), 'utf8');
    const skillFileViewerSource = await readFile(path.join(
        REPO_ROOT,
        'src/scripts/extensions/agent-system/src/skill-manager/file-viewer.js',
    ), 'utf8');

    assert.doesNotMatch(panelSource, /activeTab === 'skills'/);
    assert.doesNotMatch(panelSource, /refreshSkills/);
    assert.doesNotMatch(panelSource, /selectedSkillName/);
    assert.doesNotMatch(panelSource, /skillImport/);
    assert.doesNotMatch(panelSource, /requireSkillApi/);
    assert.doesNotMatch(panelSource, /openSkillFileViewer/);
    assert.match(skillExtensionSource, /subscribeAgentProfilesChanged/);
    assert.match(skillExtensionSource, /subscribeSettings/);
    assert.match(skillExtensionSource, /syncSelectedProfileFromSettings/);
    assert.match(skillExtensionSource, /writeFile/);
    assert.match(skillExtensionSource, /<SkillFileViewer/);
    assert.doesNotMatch(skillFileViewerSource, /showModal/);
    assert.doesNotMatch(skillFileViewerSource, /createApp/);
});

test('Agent System profile panel does not statically bundle main app modules', async () => {
    const panelSource = await readFile(path.join(
        REPO_ROOT,
        'src/scripts/extensions/agent-system/src/AgentSystemPanelApp.js',
    ), 'utf8');
    const modelTargetConnectionSource = await readFile(path.join(
        REPO_ROOT,
        'src/scripts/extensions/agent-system/src/model-target-connection.js',
    ), 'utf8');

    assert.doesNotMatch(panelSource, /preset-manager\.js/);
    assert.doesNotMatch(panelSource, /extensions\.js/);
    assert.doesNotMatch(panelSource, /script\.js/);
    assert.doesNotMatch(modelTargetConnectionSource, /preset-manager\.js/);
    assert.doesNotMatch(modelTargetConnectionSource, /extensions\.js/);
    assert.doesNotMatch(modelTargetConnectionSource, /script\.js/);
    assert.match(panelSource, /requireSillyTavernContext/);
    assert.match(modelTargetConnectionSource, /requireSillyTavernContext/);
});

test('Skill extension resolves active scoped sections from SillyTavern context', async () => {
    const hostWindow = installWindow({});
    hostWindow.SillyTavern = {
        getContext() {
            return {
                mainApi: 'openai',
                getPresetManager(apiId) {
                    assert.equal(apiId, 'openai');
                    return {
                        getSelectedPreset() {
                            return 'Creative';
                        },
                        getSelectedPresetName() {
                            return 'Creative';
                        },
                    };
                },
                characterId: 0,
                characters: [
                    {
                        name: 'Aurelia',
                        avatar: 'characters/Aurelia.png',
                    },
                ],
            };
        },
    };

    const {
        buildSkillScopeSections,
        skillScopeKey,
        skillScopeLabel,
    } = await importFresh('src/scripts/extensions/agent-system/src/skill-manager/scope.js');

    const sections = buildSkillScopeSections({
        selectedProfileId: 'writer',
        profiles: [{ id: 'writer', displayName: 'Writer' }],
    });

    assert.deepEqual(sections.map((section) => section.id), ['global', 'preset', 'profile', 'character']);
    assert.deepEqual(sections.find((section) => section.id === 'preset').scope, {
        kind: 'preset',
        apiId: 'openai',
        name: 'Creative',
    });
    assert.equal(sections.find((section) => section.id === 'preset').subtitle, 'openai / Creative');
    assert.equal(skillScopeLabel(sections.find((section) => section.id === 'preset').scope), 'Preset / Creative');
    assert.doesNotMatch(skillScopeLabel(sections.find((section) => section.id === 'preset').scope), /preset:openai/);
    assert.deepEqual(sections.find((section) => section.id === 'profile').scope, {
        kind: 'profile',
        profileId: 'writer',
    });
    assert.deepEqual(sections.find((section) => section.id === 'character').scope, {
        kind: 'character',
        characterId: 'Aurelia',
    });
    assert.equal(skillScopeKey(sections.find((section) => section.id === 'character').scope), 'character:Aurelia');
});

test('Embedded assets panel uses scoped Skill selections', async () => {
    const panelSource = await readFile(path.join(
        REPO_ROOT,
        'src/scripts/extensions/agent-system/src/embedded-assets-panel.js',
    ), 'utf8');

    assert.match(panelSource, /requireSkillApi\(\)\.list\(\{\s*scope:\s*\{\s*kind:\s*'all'\s*\}\s*\}\)/s);
    assert.match(panelSource, /selectedSkillKey/);
    assert.match(panelSource, /skillSelectionKey/);
    assert.match(panelSource, /embedSkill\(target, skill\)/);
    assert.doesNotMatch(panelSource, /selectedSkillName/);
});

test('Embedded Skill items export the selected scoped Skill archive', async () => {
    const calls = [];
    installWindow({
        skill: {
            async export(options) {
                calls.push(options);
                return {
                    fileName: 'writer.ttskill',
                    contentBase64: 'UEsDBAo=',
                    sha256: 'abc123',
                };
            },
        },
    });

    const { buildEmbeddedSkillItem } = await importFresh('src/scripts/extensions/agent-system/src/embedded-assets.js');
    const item = await buildEmbeddedSkillItem({
        name: 'writer',
        scope: { kind: 'profile', profileId: 'writer' },
    });

    assert.deepEqual(calls, [
        {
            scope: { kind: 'profile', profileId: 'writer' },
            name: 'writer',
        },
    ]);
    assert.deepEqual(item, {
        bundleFormat: 'ttskill-archive-base64-v1',
        skillName: 'writer',
        sourceScope: { kind: 'profile', profileId: 'writer' },
        sourceScopeLabel: 'Agent Profile / writer',
        fileName: 'writer.ttskill',
        contentBase64: 'UEsDBAo=',
        sha256: 'abc123',
    });
});

test('Skill extension portability sync embeds moved preset-scoped Skills', async () => {
    const exportCalls = [];
    const writes = [];
    let storedSkills = null;
    const hostWindow = installWindow({
        skill: {
            async export(options) {
                exportCalls.push(options);
                return {
                    fileName: 'writer.ttskill',
                    contentBase64: 'UEsDBAo=',
                    sha256: 'abc123',
                };
            },
        },
    });
    hostWindow.SillyTavern = {
        getContext() {
            return {
                getPresetManager(apiId) {
                    assert.equal(apiId, 'openai');
                    return {
                        getCompletionPresetByName(name) {
                            return name === 'Creative' ? { name } : null;
                        },
                        readPresetExtensionField({ name, path: fieldPath }) {
                            assert.equal(name, 'Creative');
                            assert.equal(fieldPath, 'tauritavern.skills');
                            return storedSkills;
                        },
                        async writePresetExtensionField({ name, path: fieldPath, value }) {
                            writes.push({ name, path: fieldPath, value });
                            storedSkills = value;
                        },
                    };
                },
            };
        },
    };

    const { syncSkillMovePortability } = await importFresh(
        'src/scripts/extensions/agent-system/src/skill-manager/embedded-skill-sync.js',
    );
    const presetScope = { kind: 'preset', apiId: 'openai', name: 'Creative' };
    await syncSkillMovePortability(
        {
            name: 'writer',
            fromScope: { kind: 'global' },
            toScope: presetScope,
        },
        {
            action: 'installed',
            name: 'writer',
            scope: presetScope,
        },
    );

    assert.deepEqual(exportCalls, [{ scope: presetScope, name: 'writer' }]);
    assert.equal(writes.length, 1);
    assert.deepEqual(storedSkills, {
        version: 1,
        items: [
            {
                bundleFormat: 'ttskill-archive-base64-v1',
                skillName: 'writer',
                sourceScope: presetScope,
                sourceScopeLabel: 'Preset / Creative',
                fileName: 'writer.ttskill',
                contentBase64: 'UEsDBAo=',
                sha256: 'abc123',
            },
        ],
    });
});

test('Skill extension portability sync writes character embedded Skills without edit-form coupling', async () => {
    const previousFetch = globalThis.fetch;
    const previousDocument = globalThis.document;
    delete globalThis.document;

    const fetchCalls = [];
    globalThis.fetch = async (url, options) => {
        fetchCalls.push({
            url,
            body: JSON.parse(options.body),
        });
        return {
            ok: true,
            text: async () => '',
        };
    };

    try {
        const character = {
            name: 'Aurelia',
            avatar: 'characters/Aurelia.png',
            data: {
                extensions: {
                    tauritavern: {
                        agentProfiles: {
                            version: 1,
                            items: [{ profile: { id: 'stale-local-profile' } }],
                        },
                    },
                },
            },
            json_data: JSON.stringify({
                data: {
                    extensions: {
                        tauritavern: {
                            agentProfiles: {
                                version: 1,
                                items: [{ profile: { id: 'stale-local-profile' } }],
                            },
                        },
                    },
                },
            }),
        };
        const hostWindow = installWindow({
            skill: {
                async export() {
                    return {
                        fileName: 'writer.ttskill',
                        contentBase64: 'UEsDBAo=',
                        sha256: 'abc123',
                    };
                },
            },
        });
        hostWindow.SillyTavern = {
            getContext() {
                return {
                    characters: [character],
                    getRequestHeaders() {
                        return { 'content-type': 'application/json' };
                    },
                };
            },
        };

        const { syncSkillWritePortability } = await importFresh(
            'src/scripts/extensions/agent-system/src/skill-manager/embedded-skill-sync.js',
        );
        await syncSkillWritePortability({
            scope: { kind: 'character', characterId: 'Aurelia' },
            name: 'writer',
        });

        assert.equal(fetchCalls.length, 1);
        assert.equal(fetchCalls[0].url, '/api/characters/merge-attributes');
        assert.equal(fetchCalls[0].body.avatar, 'Aurelia.png');
        assert.deepEqual(Object.keys(fetchCalls[0].body.data.extensions.tauritavern), ['skills']);
        assert.equal(
            character.data.extensions.tauritavern.skills.items[0].contentBase64,
            'UEsDBAo=',
        );
        assert.equal(
            character.data.extensions.tauritavern.agentProfiles.items[0].profile.id,
            'stale-local-profile',
        );
    } finally {
        globalThis.fetch = previousFetch;
        if (previousDocument === undefined) {
            delete globalThis.document;
        } else {
            globalThis.document = previousDocument;
        }
    }
});

test('Agent System stylesheet drops legacy profile-tab Skill selectors', async () => {
    const css = await readFile(path.join(
        REPO_ROOT,
        'src/scripts/extensions/agent-system/style.css',
    ), 'utf8');

    for (const selector of [
        'ttas-skill-hero',
        'ttas-skill-pane',
        'ttas-skill-meta',
        'ttas-tags',
        'ttas-import-summary',
        'ttas-warning-list',
        'ttas-details',
    ]) {
        assert.doesNotMatch(css, new RegExp(`\\.${selector}\\b`));
    }
});

test('Skill extension marks unsaved GUI presets unavailable instead of inventing a scope', async () => {
    const hostWindow = installWindow({});
    hostWindow.SillyTavern = {
        getContext() {
            return {
                mainApi: 'openai',
                getPresetManager() {
                    return {
                        getSelectedPreset() {
                            return 'gui';
                        },
                        getSelectedPresetName() {
                            return 'Unsaved GUI Draft';
                        },
                    };
                },
                characterId: undefined,
                characters: [],
            };
        },
    };

    const { buildSkillScopeSections } = await importFresh('src/scripts/extensions/agent-system/src/skill-manager/scope.js');
    const sections = buildSkillScopeSections({
        selectedProfileId: 'writer',
        profiles: [{ id: 'writer', displayName: 'Writer' }],
    });
    const preset = sections.find((section) => section.id === 'preset');

    assert.equal(preset.available, false);
    assert.equal(preset.scope, null);
});

test('PromptManager materializes reserved Agent prompts at PromptManager positions', async () => {
    const promptManagerSource = await readFile(path.join(REPO_ROOT, 'src/scripts/PromptManager.js'), 'utf8');
    const openAiSource = await readFile(path.join(REPO_ROOT, 'src/scripts/openai.js'), 'utf8');

    assert.match(promptManagerSource, /const AGENT_SYSTEM_PROMPT_IDENTIFIER = 'agentSystemPrompt';/);
    assert.match(promptManagerSource, /const AGENT_RESULTS_PROMPT_IDENTIFIER = 'agentResults';/);
    assert.match(promptManagerSource, /const AGENT_TASK_PROMPT_IDENTIFIER = 'agentTask';/);
    assert.match(promptManagerSource, /normalizeAgentPromptRole/);
    assert.match(promptManagerSource, /normalizeAgentPromptMarkerDefinitions\(\)/);
    assert.match(promptManagerSource, /normalizeAgentSystemPromptDefinition\(\)/);
    assert.match(promptManagerSource, /normalizeAgentTaskPromptDefinition\(\)/);
    assert.match(promptManagerSource, /normalizeAgentResultsPromptDefinition\(\)/);
    assert.match(promptManagerSource, /agent\.task_prompt_definition_missing/);
    assert.match(promptManagerSource, /agent\.results_prompt_definition_missing/);
    assert.match(promptManagerSource, /marker:\s*true/);
    assert.doesNotMatch(promptManagerSource, /existing\.enabled\s*=\s*true/);
    assert.doesNotMatch(promptManagerSource, /case 'agentSystemPrompt':/);

    assert.match(openAiSource, /populateAgentSystemPrompt/);
    assert.match(openAiSource, /populateAgentTaskPrompt/);
    assert.match(openAiSource, /agentTaskPrompt/);
    assert.match(openAiSource, /Message\.fromPromptAsync\(materializedPrompt,\s*assemblyRuntime\.tokenHandler\)/);
    assert.doesNotMatch(openAiSource, /_tauritavern_agent_prompt_marker/);
    assert.doesNotMatch(openAiSource, /populateAgentSystemPromptMarker/);
    assert.doesNotMatch(openAiSource, /populateAgentResults/);
    assert.doesNotMatch(openAiSource, /\[Agent Result\]/);
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

test('Agent run controller treats partial success as a terminal resolved run', async () => {
    let listener = null;
    installWindow({
        agent: {
            async startRunWithPromptSnapshot(input) {
                return { runId: 'run-partial', input };
            },
            subscribe(runId, callback) {
                assert.equal(runId, 'run-partial');
                listener = callback;
                return () => {};
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

    listener({
        type: 'run_partial_success',
        payload: {
            code: 'model.tool_call_required',
            message: 'model must use Agent tools',
            retryable: false,
            userRetryable: false,
            preservedCommitCount: 1,
            preservedCommits: [{ path: 'output/main.md', mode: 'replace', messageId: '1', round: 2 }],
        },
    });
    const result = await run;
    unsubscribe();

    assert.equal(result.handle.runId, 'run-partial');
    assert.equal(result.terminalEvent.type, 'run_partial_success');
    assert.equal(controller.hasActiveAgentRun(), false);
    assert.equal(stateChanges.at(-1).lastEvent.type, 'run_partial_success');
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

    const recoveryEvent = {
        seq: 5,
        id: 'evt-recovery',
        runId: 'run-1',
        type: 'drift_recovery_attempted',
        level: 'warn',
        payload: {
            attempt: 1,
            maxAttempts: 1,
            reasonCode: 'model.tool_call_required',
        },
    };
    assert.equal(presenter.isDisplayableRunEvent(recoveryEvent), true);
    const recoveryItem = presenter.presentRunEvent(recoveryEvent);
    assert.equal(recoveryItem.titleKey, 'timelineEventDriftRecoveryAttempted');
    assert.deepEqual(recoveryItem.titleParams, { attempt: 1, max: 1 });
    assert.equal(recoveryItem.summary, 'model.tool_call_required');

    const directOutputEvent = {
        seq: 6,
        id: 'evt-direct-output',
        runId: 'run-1',
        type: 'direct_output_captured',
        level: 'warn',
        payload: {
            round: 2,
            path: 'output/direct_output.md',
            chars: 32,
            words: 6,
        },
    };
    assert.equal(presenter.isDisplayableRunEvent(directOutputEvent), true);
    const directOutputItem = presenter.presentRunEvent(directOutputEvent);
    assert.equal(directOutputItem.titleKey, 'timelineEventDirectOutputCaptured');
    assert.deepEqual(directOutputItem.titleParams, { path: 'output/direct_output.md' });
    assert.equal(directOutputItem.summary, '32 chars / 6 words');

    const readCompletedEvent = {
        seq: 7,
        id: 'evt-read-completed',
        runId: 'run-1',
        type: 'tool_call_completed',
        level: 'info',
        payload: {
            callId: 'call-read',
            name: 'workspace.read_file',
            displayMetrics: {
                chars: 48,
                words: 9,
            },
            resourceRefs: ['output/main.md'],
        },
    };
    assert.equal(presenter.presentRunEvent(readCompletedEvent).summary, '48 chars / 9 words');

    const partialEvent = {
        seq: 8,
        id: 'evt-partial',
        runId: 'run-1',
        type: 'run_partial_success',
        level: 'warn',
        payload: {
            code: 'model.tool_call_required',
            message: 'model must use tools',
            preservedCommitCount: 1,
            preservedCommits: [{ path: 'output/main.md', mode: 'replace', messageId: '3', round: 4 }],
        },
    };
    assert.equal(presenter.isDisplayableRunEvent(partialEvent), true);
    const partialItem = presenter.presentRunEvent(partialEvent);
    assert.equal(partialItem.titleKey, 'timelineEventRunPartialSuccess');
    assert.deepEqual(partialItem.titleParams, { count: 1 });
    assert.equal(partialItem.tone, 'warn');
    assert.equal(partialItem.summary, '1 committed message preserved');

    const commitRequestedEvent = {
        seq: 9,
        id: 'evt-commit-requested',
        runId: 'run-1',
        type: 'chat_commit_requested',
        payload: {
            commitId: 'commit-1',
            path: 'output/main.md',
            mode: 'replace',
            chars: 64,
            words: 12,
        },
    };
    const commitCompletedEvent = {
        seq: 10,
        id: 'evt-commit-completed',
        runId: 'run-1',
        type: 'chat_commit_completed',
        payload: {
            commitId: 'commit-1',
            path: 'output/main.md',
            mode: 'replace',
            messageId: '4',
        },
    };
    const commitItems = presenter.timelineItemsFromEvents([commitRequestedEvent, commitCompletedEvent]);
    assert.deepEqual(commitItems.map(item => item.type), ['chat_commit_completed']);
    assert.equal(commitItems[0].summary, 'message 4 | 64 chars / 12 words');

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
            payload: { path: 'output/main.md', chars: 12, words: 2 },
        },
        directOutputEvent,
    ]);
    assert.deepEqual(projected.map(event => event.type), ['workspace_file_written', 'direct_output_captured']);
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
        payload: { path: 'output/main.md', chars: 12, words: 2 },
    };
    const writeTargets = presenter.buildEventDetailTargets(
        presenter.presentRunEvent(writeEvent),
        [resultEvent, completed, writeEvent],
    );

    assert.deepEqual(writeTargets.map(target => [target.type, target.labelKey, target.path || '']), [
        ['file', 'timelineWorkspaceFile', 'output/main.md'],
    ]);
    assert.equal(writeTargets[0].chars, 12);
    assert.equal(writeTargets[0].words, 2);

    const directOutputEvent = {
        seq: 4,
        id: 'evt-direct-output',
        runId: 'run-1',
        type: 'direct_output_captured',
        payload: { path: 'output/direct_output.md', chars: 32, words: 6 },
    };
    const directOutputTargets = presenter.buildEventDetailTargets(
        presenter.presentRunEvent(directOutputEvent),
        [resultEvent, completed, writeEvent, directOutputEvent],
    );

    assert.deepEqual(directOutputTargets.map(target => [target.type, target.labelKey, target.path || '']), [
        ['file', 'timelineWorkspaceFile', 'output/direct_output.md'],
    ]);
    assert.equal(directOutputTargets[0].chars, 32);
    assert.equal(directOutputTargets[0].words, 6);

    const patchRequested = {
        seq: 5,
        id: 'evt-patch-requested',
        runId: 'run-1',
        type: 'tool_call_requested',
        payload: {
            callId: 'call-2',
            name: 'workspace.apply_patch',
            argumentsRef: 'tool-args/call-2.json',
        },
    };
    const patchCompleted = {
        seq: 6,
        id: 'evt-patch-completed',
        runId: 'run-1',
        type: 'tool_call_completed',
        payload: {
            callId: 'call-2',
            name: 'workspace.apply_patch',
            resourceRefs: ['output/main.md'],
        },
    };
    const patchEvent = {
        seq: 7,
        id: 'evt-patch',
        runId: 'run-1',
        type: 'workspace_patch_applied',
        payload: { path: 'output/main.md', chars: 24, words: 4, replacements: 1 },
    };
    const patchTargets = presenter.buildEventDetailTargets(
        presenter.presentRunEvent(patchEvent),
        [patchRequested, patchCompleted, patchEvent],
    );

    assert.deepEqual(patchTargets.map(target => [target.type, target.labelKey, target.path || '', target.argumentsRef || '']), [
        ['patchDiff', 'timelinePatchDiff', 'output/main.md', 'tool-args/call-2.json'],
        ['file', 'timelineWorkspaceFile', 'output/main.md', ''],
    ]);
    assert.equal(patchTargets[0].chars, 24);
    assert.equal(patchTargets[0].words, 4);
    assert.equal(patchTargets[1].chars, 24);
    assert.equal(patchTargets[1].words, 4);
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
            reasoningChars: 30,
            reasoningWords: 5,
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
    assert.deepEqual(presenter.timelineItemsFromEvents([modelEvent], { includeModelTurns: true }).map(item => item.type), []);

    const targets = presenter.buildEventDetailTargets(
        presenter.presentRunEvent(toolEvent),
        [modelEvent, toolEvent],
    );
    assert.deepEqual(targets, [
        { type: 'modelReasoning', labelKey: 'timelineReasoning', round: 2 },
    ]);
});

test('Agent run event presenter restores reasoning for collapsed side-effect events', async () => {
    const presenter = await importFresh('src/scripts/extensions/agent-system/src/run-event-presenter.js');
    const modelEvent = {
        seq: 1,
        id: 'evt-model',
        runId: 'run-1',
        type: 'model_completed',
        payload: {
            round: 7,
            hasReasoning: true,
            reasoningChars: 48,
            reasoningWords: 8,
        },
    };
    const writeCompleted = {
        seq: 2,
        id: 'evt-write-completed',
        runId: 'run-1',
        type: 'tool_call_completed',
        payload: {
            round: 7,
            callId: 'call-write',
            name: 'workspace.write_file',
            resourceRefs: ['output/main.md'],
        },
    };
    const writeEvent = {
        seq: 3,
        id: 'evt-write',
        runId: 'run-1',
        type: 'workspace_file_written',
        payload: { path: 'output/main.md', chars: 12, words: 2 },
    };
    const commitRequestedTool = {
        seq: 4,
        id: 'evt-commit-tool',
        runId: 'run-1',
        type: 'tool_call_requested',
        payload: {
            round: 7,
            callId: 'call-commit',
            name: 'workspace.commit',
            argumentsRef: 'tool-args/call-commit.json',
        },
    };
    const commitEvent = {
        seq: 5,
        id: 'evt-commit',
        runId: 'run-1',
        type: 'chat_commit_completed',
        payload: {
            callId: 'call-commit',
            commitId: 'commit-1',
            path: 'output/main.md',
            mode: 'replace',
        },
    };
    const patchRequested = {
        seq: 6,
        id: 'evt-patch-requested',
        runId: 'run-1',
        type: 'tool_call_requested',
        payload: {
            round: 7,
            callId: 'call-patch',
            name: 'workspace.apply_patch',
            argumentsRef: 'tool-args/call-patch.json',
        },
    };
    const patchCompleted = {
        seq: 7,
        id: 'evt-patch-completed',
        runId: 'run-1',
        type: 'tool_call_completed',
        payload: {
            round: 7,
            callId: 'call-patch',
            name: 'workspace.apply_patch',
            resourceRefs: ['output/main.md'],
        },
    };
    const patchEvent = {
        seq: 8,
        id: 'evt-patch',
        runId: 'run-1',
        type: 'workspace_patch_applied',
        payload: { path: 'output/main.md', chars: 24, words: 4, replacements: 1 },
    };
    const finishCompleted = {
        seq: 9,
        id: 'evt-finish-completed',
        runId: 'run-1',
        type: 'tool_call_completed',
        payload: {
            round: 7,
            callId: 'call-finish',
            name: 'workspace.finish',
        },
    };
    const persistentEvent = {
        seq: 10,
        id: 'evt-persistent',
        runId: 'run-1',
        type: 'persistent_changes_committed',
        payload: { changeCount: 0, changes: [] },
    };
    const events = [
        modelEvent,
        writeCompleted,
        writeEvent,
        commitRequestedTool,
        commitEvent,
        patchRequested,
        patchCompleted,
        patchEvent,
        finishCompleted,
        persistentEvent,
    ];

    const writeTargets = presenter.buildEventDetailTargets(presenter.presentRunEvent(writeEvent), events);
    assert.deepEqual(writeTargets[0], { type: 'modelReasoning', labelKey: 'timelineReasoning', round: 7 });

    const commitTargets = presenter.buildEventDetailTargets(presenter.presentRunEvent(commitEvent), events);
    assert.deepEqual(commitTargets[0], { type: 'modelReasoning', labelKey: 'timelineReasoning', round: 7 });

    const patchTargets = presenter.buildEventDetailTargets(presenter.presentRunEvent(patchEvent), events);
    assert.deepEqual(patchTargets[0], { type: 'modelReasoning', labelKey: 'timelineReasoning', round: 7 });
    assert.equal(patchTargets[1].type, 'patchDiff');

    const persistentTargets = presenter.buildEventDetailTargets(presenter.presentRunEvent(persistentEvent), events);
    assert.deepEqual(persistentTargets, [
        { type: 'modelReasoning', labelKey: 'timelineReasoning', round: 7 },
    ]);
});

test('Agent run detail formatter renders tool result details without raw JSON shell', async () => {
    const { formatDetailFile } = await importFresh('src/scripts/extensions/agent-system/src/run-detail-format.js');
    const section = formatDetailFile(
        { labelKey: 'timelineToolResult', path: 'tool-results/call-1.json' },
        {
            path: 'tool-results/call-1.json',
            chars: 248,
            words: 32,
            sha256: '0123456789abcdef0123456789abcdef',
            text: JSON.stringify({
                callId: 'call-1',
                name: 'workspace.read_file',
                content: 'output/main.md lines 1-2 of 2, chars 0-11 of 11, words 2 of 2, sha256 abc\n1 hello\n2 world',
                structured: {
                    path: 'output/main.md',
                    totalLines: 2,
                    startLine: 1,
                    endLine: 2,
                    chars: 11,
                    words: 2,
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
        { label: 'Text', value: '11 chars / 2 words' },
    ]);
});

test('Agent run detail formatter renders apply_patch arguments as red green diff rows', async () => {
    const { formatPatchDiffDetail } = await importFresh('src/scripts/extensions/agent-system/src/run-detail-format.js');
    const section = formatPatchDiffDetail(
        {
            type: 'patchDiff',
            labelKey: 'timelinePatchDiff',
            path: 'output/main.md',
            argumentsRef: 'tool-args/call-2.json',
            replacements: 1,
            chars: 24,
            words: 4,
        },
        {
            path: 'tool-args/call-2.json',
            text: JSON.stringify({
                path: 'output/main.md',
                old_string: 'alpha\nold\nomega',
                new_string: 'alpha\nnew\nomega',
            }),
        },
    );

    assert.equal(section.labelKey, 'timelinePatchDiff');
    assert.deepEqual(section.fields, [
        { label: 'Target', value: 'output/main.md' },
        { label: 'Replacements', value: '1' },
        { label: 'Text', value: '24 chars / 4 words' },
    ]);
    assert.deepEqual(section.blocks[0].rows, [
        { type: 'context', oldLine: 1, newLine: 1, marker: ' ', text: 'alpha' },
        { type: 'delete', oldLine: 2, newLine: null, marker: '-', text: 'old' },
        { type: 'add', oldLine: null, newLine: 2, marker: '+', text: 'new' },
        { type: 'context', oldLine: 3, newLine: 3, marker: ' ', text: 'omega' },
    ]);
    assert.equal(section.blocks[0].meta, '+1 / -1');
});

test('Agent run detail formatter shows workspace file text metrics', async () => {
    const { formatDetailFile } = await importFresh('src/scripts/extensions/agent-system/src/run-detail-format.js');
    const section = formatDetailFile(
        {
            labelKey: 'timelineWorkspaceFile',
            path: 'output/main.md',
            chars: 12,
            words: 2,
        },
        {
            path: 'output/main.md',
            chars: 15,
            words: 3,
            sha256: 'abc',
            text: 'hello world',
        },
    );

    assert.equal(section.labelKey, 'timelineWorkspaceFile');
    assert.deepEqual(section.fields, [
        { label: 'Text', value: '12 chars / 2 words' },
    ]);
    assert.equal(section.blocks[0].labelKey, 'timelineContent');
    assert.equal(section.blocks[0].text, 'hello world');
});

test('Agent run detail formatter renders model turn display DTO', async () => {
    const { formatModelTurnDetail } = await importFresh('src/scripts/extensions/agent-system/src/run-detail-format.js');
    const turn = {
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
            totalChars: 29,
            totalWords: 5,
            truncated: false,
        },
        reasoning: [{
            source: 'reasoning_content',
            text: 'Need to inspect the workspace.',
            totalChars: 30,
            totalWords: 5,
            truncated: true,
        }],
        toolCalls: [{
            callId: 'call-1',
            name: 'workspace.read_file',
        }],
    };
    const section = formatModelTurnDetail(
        { type: 'modelReasoning', labelKey: 'timelineReasoning', round: 2 },
        turn,
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
    assert.equal(section.blocks[0].meta, 'reasoning_content · 30 chars / 5 words');
    assert.match(section.blocks[0].text, /Need to inspect/);

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
});

test('Agent error presenter translation keys exist in global locales', async () => {
    const source = await readFile(path.join(
        REPO_ROOT,
        'src/scripts/tauritavern/agent/agent-error-presenter.js',
    ), 'utf8');
    const keys = [...new Set([...source.matchAll(/'((?:agent\.error\.)[^']+)'/g)]
        .map(match => match[1]))];
    assert.ok(keys.length > 0, 'expected Agent error translation keys');

    for (const locale of ['en', 'zh-cn', 'zh-tw']) {
        const messages = JSON.parse(await readFile(path.join(
            REPO_ROOT,
            `src/locales/${locale}.json`,
        ), 'utf8'));
        for (const key of keys) {
            assert.ok(Object.hasOwn(messages, key), `${locale} missing ${key}`);
        }
    }
});

test('Run failure detail surfaces a retry action and userRetryable field when allowed', async () => {
    const { formatRunFailureDetail } = await importFresh('src/scripts/extensions/agent-system/src/run-detail-format.js');

    const drift = formatRunFailureDetail({
        labelKey: 'timelineErrorDetails',
        event: {
            payload: {
                code: 'agent.tool_after_finish',
                message: 'fatal',
                technicalMessage: 'Validation error: agent.tool_after_finish',
                retryable: false,
                userRetryable: true,
            },
        },
    });
    const userRetryableField = drift.fields.find(field => field.label === 'User-retryable');
    assert.ok(userRetryableField, 'user-retryable field must be present');
    assert.equal(userRetryableField.value, 'true');
    assert.deepEqual(drift.actions.map(action => action.kind), ['retry']);

    const fatal = formatRunFailureDetail({
        labelKey: 'timelineErrorDetails',
        event: { payload: { code: 'agent.internal_error', message: 'boom', retryable: false } },
    });
    assert.deepEqual(fatal.actions, []);
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

test('Agent timeline retry action does not invoke the SillyTavern regenerate DOM button', async () => {
    const source = await readFile(path.join(
        REPO_ROOT,
        'src/scripts/extensions/agent-system/src/run-timeline-panel.js',
    ), 'utf8');

    assert.match(source, /retryAgentRunFailure/);
    assert.doesNotMatch(source, /option_regenerate/);
    assert.doesNotMatch(source, /globalThis\.jQuery|globalThis\.\$/);
});

test('Partial success detail keeps error visible without retry action', async () => {
    const { formatRunFailureDetail } = await importFresh('src/scripts/extensions/agent-system/src/run-detail-format.js');

    const section = formatRunFailureDetail({
        labelKey: 'timelineErrorDetails',
        event: {
            type: 'run_partial_success',
            payload: {
                code: 'model.tool_call_required',
                message: 'model must use Agent tools',
                technicalMessage: 'Validation error: model.tool_call_required',
                retryable: false,
                userRetryable: false,
                preservedCommitCount: 1,
                preservedCommits: [{ path: 'output/main.md', mode: 'replace', messageId: '3', round: 4 }],
            },
        },
    });

    assert.deepEqual(section.actions, []);
    assert.deepEqual(section.fields, [
        { label: 'Error', value: 'model.tool_call_required' },
        { label: 'Preserved commits', value: '1' },
        { label: 'Retryable', value: 'false' },
        { label: 'User-retryable', value: 'false' },
    ]);
    assert.equal(section.blocks[0].labelKey, 'timelinePartialSuccessMessage');
    assert.match(section.blocks[0].text, /kept committed chat output/);
    assert.equal(section.blocks[1].labelKey, 'timelineResultText');
    assert.equal(section.blocks[1].text, 'model must use Agent tools');
});

test('Agent run controller awaits rollback before rejecting on drift run_failed', async () => {
    let listener = null;
    installWindow({
        agent: {
            async startRunWithPromptSnapshot() {
                return { runId: 'run-drift' };
            },
            subscribe(_runId, callback) {
                listener = callback;
                return () => {};
            },
        },
    });

    const controller = await importFresh('src/scripts/tauritavern/agent/agent-run-controller.js');
    const deletions = [];
    const updates = [];
    let resolveRollback;
    const rollbackGate = new Promise((resolve) => {
        resolveRollback = resolve;
    });
    const chat = [
        {},
        { extra: { tauritavern: { agent: { runId: 'run-drift', rollback: { strategy: 'deleteMessage' } } } } },
    ];
    const rollbackScript = {
        chat,
        async deleteMessage(index) {
            deletions.push(index);
            await rollbackGate;
            chat.splice(index, 1);
        },
    };
    installRollbackEventCapture(rollbackScript, updates);
    controller.__setAgentRunRollbackScriptForTests(rollbackScript);

    const run = controller.startAndWaitForAgentRun({ generationType: 'normal' });
    await Promise.resolve();

    listener({
        seq: 1,
        runId: 'run-drift',
        type: 'run_rollback_targets',
        payload: {
            reasonCode: 'model.tool_call_required',
            targets: [{ path: 'output/main.md', mode: 'replace', messageId: '1', round: 1 }],
        },
    });
    listener({
        seq: 2,
        runId: 'run-drift',
        type: 'run_failed',
        payload: {
            code: 'model.tool_call_required',
            message: 'drift',
            retryable: false,
            userRetryable: true,
        },
    });

    let settled = false;
    void run.catch(() => { settled = true; });
    await Promise.resolve();
    await Promise.resolve();
    assert.equal(settled, false, 'run must wait for rollback to complete');

    resolveRollback();
    await assert.rejects(() => run, (error) => {
        assert.equal(error.userRetryable, true);
        assert.equal(error.retryable, false);
        assert.equal(error.agentErrorCode, 'model.tool_call_required');
        return true;
    });
    assert.deepEqual(deletions, [1]);
    assert.deepEqual(updates, [{ event: 'message_updated', messageId: 0 }]);
    controller.__setAgentRunRollbackScriptForTests(null);
});

test('Agent run controller rejects rollback failures before presenting drift failure', async () => {
    let listener = null;
    installWindow({
        agent: {
            async startRunWithPromptSnapshot() {
                return { runId: 'run-drift-rollback-fails' };
            },
            subscribe(_runId, callback) {
                listener = callback;
                return () => {};
            },
        },
    });

    const controller = await importFresh('src/scripts/tauritavern/agent/agent-run-controller.js');
    const rollbackScript = {
        chat: [
            {},
            { extra: { tauritavern: { agent: { runId: 'run-drift-rollback-fails', rollback: { strategy: 'deleteMessage' } } } } },
        ],
        async deleteMessage() {
            throw new Error('delete failed');
        },
    };
    installRollbackEventCapture(rollbackScript);
    controller.__setAgentRunRollbackScriptForTests(rollbackScript);

    const run = controller.startAndWaitForAgentRun({ generationType: 'normal' });
    await Promise.resolve();

    listener({
        seq: 1,
        runId: 'run-drift-rollback-fails',
        type: 'run_rollback_targets',
        payload: {
            reasonCode: 'model.tool_call_required',
            targets: [{ path: 'output/main.md', mode: 'replace', messageId: '1', round: 1 }],
        },
    });
    listener({
        seq: 2,
        runId: 'run-drift-rollback-fails',
        type: 'run_failed',
        payload: {
            code: 'model.tool_call_required',
            message: 'drift',
            retryable: false,
            userRetryable: true,
        },
    });

    await assert.rejects(() => run, (error) => {
        assert.equal(error.name, 'AgentRunRollbackError');
        assert.equal(error.agentErrorCode, 'agent.rollback_failed');
        assert.match(error.message, /delete failed/);
        assert.equal(error.userRetryable, false);
        return true;
    });
    controller.__setAgentRunRollbackScriptForTests(null);
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

test('Rollback helper fails fast on invalid or foreign targets', async () => {
    const { rollbackAgentRunDriftMessages } = await importFresh('src/scripts/tauritavern/agent/agent-run-message-rollback.js');

    await assert.rejects(
        () => rollbackAgentRunDriftMessages({
            runId: 'run-x',
            targets: [{ messageId: 'invalid' }],
            script: { chat: [], async deleteMessage() {} },
        }),
        /agent\.rollback_target_invalid/,
    );

    await assert.rejects(
        () => rollbackAgentRunDriftMessages({
            runId: 'run-x',
            targets: [{ messageId: '0' }],
            script: installRollbackEventCapture({
                chat: [{ extra: { tauritavern: { agent: { runId: 'other-run' } } } }],
                async deleteMessage() {},
            }),
        }),
        /agent\.rollback_run_mismatch/,
    );
});

test('Rollback helper pops only the run-added swipe when the message pre-existed', async () => {
    const { rollbackAgentRunDriftMessages } = await importFresh('src/scripts/tauritavern/agent/agent-run-message-rollback.js');

    const chat = [
        { is_user: true, mes: 'hello' },
        {
            is_user: false,
            mes: 'agent drift attempt',
            swipe_id: 2,
            swipes: ['user-authored', 'user-authored alt 1', 'agent drift attempt'],
            swipe_info: [
                { extra: {} },
                { extra: {} },
                { extra: { tauritavern: { agent: { runId: 'run-swipe' } } } },
            ],
            extra: {
                tauritavern: {
                    agent: {
                        runId: 'run-swipe',
                        rollback: { strategy: 'deleteSwipe', swipeId: 2 },
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
            chat[messageId].swipes.splice(swipeId, 1);
            chat[messageId].swipe_info.splice(swipeId, 1);
            chat[messageId].swipe_id = Math.min(swipeId, chat[messageId].swipes.length - 1);
            chat[messageId].mes = chat[messageId].swipes[chat[messageId].swipe_id];
            return chat[messageId].swipe_id;
        },
        async deleteMessage(index) {
            messageDeletions.push(index);
            chat.splice(index, 1);
        },
    };
    installRollbackEventCapture(script, updates);

    const result = await rollbackAgentRunDriftMessages({
        runId: 'run-swipe',
        targets: [{ messageId: '1' }, { messageId: '1' }],
        script,
    });

    assert.deepEqual(swipeCalls, [{ swipeId: 2, messageId: 1 }]);
    assert.deepEqual(messageDeletions, []);
    assert.equal(result.swipesRemoved, 1);
    assert.equal(result.deleted, 0);
    assert.equal(chat.length, 2, 'pre-existing message must be preserved');
    assert.equal(chat[1].mes, 'user-authored alt 1');
    assert.deepEqual(chat[1].swipes, ['user-authored', 'user-authored alt 1']);
    assert.deepEqual(updates, [{ event: 'message_updated', messageId: 1 }]);
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

test('Rollback helper fails fast when deleteMessage leaves the target in chat', async () => {
    const { rollbackAgentRunDriftMessages } = await importFresh('src/scripts/tauritavern/agent/agent-run-message-rollback.js');

    const chat = [
        { extra: { tauritavern: { agent: { runId: 'run-noop-delete', rollback: { strategy: 'deleteMessage' } } } } },
    ];
    const updates = [];

    await assert.rejects(
        () => rollbackAgentRunDriftMessages({
            runId: 'run-noop-delete',
            targets: [{ messageId: '0' }],
            script: installRollbackEventCapture({
                chat,
                async deleteMessage() {},
            }, updates),
        }),
        /agent\.rollback_message_delete_failed/,
    );

    assert.equal(chat.length, 1);
    assert.deepEqual(updates, []);
});

test('Rollback helper requires MESSAGE_UPDATED before destructive rollback', async () => {
    const { rollbackAgentRunDriftMessages } = await importFresh('src/scripts/tauritavern/agent/agent-run-message-rollback.js');

    const chat = [
        { extra: { tauritavern: { agent: { runId: 'run-no-events', rollback: { strategy: 'deleteMessage' } } } } },
    ];
    const deletions = [];
    await assert.rejects(
        () => rollbackAgentRunDriftMessages({
            runId: 'run-no-events',
            targets: [{ messageId: '0' }],
            script: {
                chat,
                async deleteMessage(index) {
                    deletions.push(index);
                    chat.splice(index, 1);
                },
            },
        }),
        /agent\.rollback_event_api_unavailable/,
    );

    assert.deepEqual(deletions, []);
    assert.equal(chat.length, 1, 'rollback must fail before mutating chat when update events are unavailable');
});

test('Rollback helper requires rollback strategy and host APIs for targeted messages', async () => {
    const { rollbackAgentRunDriftMessages } = await importFresh('src/scripts/tauritavern/agent/agent-run-message-rollback.js');

    await assert.rejects(
        () => rollbackAgentRunDriftMessages({
            runId: 'run-missing-strategy',
            targets: [{ messageId: '0' }],
            script: installRollbackEventCapture({
                chat: [{ extra: { tauritavern: { agent: { runId: 'run-missing-strategy' } } } }],
                async deleteMessage() {},
            }),
        }),
        /agent\.rollback_strategy_missing/,
    );

    await assert.rejects(
        () => rollbackAgentRunDriftMessages({
            runId: 'run-swipe-no-api',
            targets: [{ messageId: '0' }],
            script: installRollbackEventCapture({
                chat: [{
                    swipes: ['old', 'new'],
                    extra: {
                        tauritavern: {
                            agent: {
                                runId: 'run-swipe-no-api',
                                rollback: { strategy: 'deleteSwipe', swipeId: 1 },
                            },
                        },
                    },
                }],
                async deleteMessage() {},
            }),
        }),
        /agent\.rollback_host_api_unavailable: deleteSwipe/,
    );
});
