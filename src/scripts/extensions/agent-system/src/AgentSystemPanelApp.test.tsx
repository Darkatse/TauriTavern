import { act, cleanup, render, screen, waitFor } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { afterEach, expect, test } from '@rstest/core';

import { AgentSystemPanelApp } from './AgentSystemPanelApp';
import {
    createAgentSystemPanelController,
    type AgentSystemPanelController,
} from './AgentSystemPanelController';
import type { AgentSystemPanelControllerDeps } from './AgentSystemPanelContract';
import { createRunHistoryController } from './RunHistoryController';
import { createRunRetentionController } from './RunRetentionController';
import { defaultProfile } from './profile-model';
import type { AgentSystemSettings } from './AgentSystemEntryController';

type ProfileListResult = Awaited<ReturnType<TauriTavernAgentProfilesApi['list']>>;

function formatParam(value: unknown): string {
    if (typeof value === 'string' || typeof value === 'number' || typeof value === 'boolean') {
        return String(value);
    }
    return JSON.stringify(value) ?? '';
}

const tr = (key: string, params: Record<string, unknown> = {}): string => (
    [key, ...Object.entries(params).map(([name, value]) => `${name}=${formatParam(value)}`)].join(' ')
);

function settings(editingProfileId: string): AgentSystemSettings {
    return {
        agentModeEnabled: true,
        chatInputToggleHidden: false,
        activeProfileId: 'default-writer',
        editingProfileId,
        activeTab: 'profiles',
        runTimelineHeightPx: null,
    };
}

function summary(profile: TauriTavernAgentProfileDefinition): TauriTavernAgentProfileSummary {
    return {
        id: profile.id,
        displayName: profile.displayName,
        ...(profile.description !== undefined ? { description: profile.description } : {}),
        directRunnable: profile.run.directRunnable,
    };
}

function healthyProfile(profileId: string): TauriTavernAgentProfileHealth {
    return {
        profileId,
        previewAvailable: true,
        promptAssemblyAvailable: true,
        directRunAvailable: true,
        subAgentAvailable: true,
        diagnostics: [],
    };
}

function createPanelWorld(selectedProfile = defaultProfile()) {
    const definitions = new Map<string, TauriTavernAgentProfileDefinition>();
    const builtin = defaultProfile();
    definitions.set(builtin.id, builtin);
    definitions.set(selectedProfile.id, selectedProfile);

    const state = {
        settings: settings(selectedProfile.id),
        definitions,
        listResults: [] as ProfileListResult[],
        listCount: 0,
        repairs: [] as Array<{ profileId: string; action: 'delete' | 'normalizeIdentity' }>,
        confirmations: [] as string[],
        confirm: true,
        warnings: [] as string[],
        errors: [] as string[],
        saves: [] as TauriTavernAgentProfileDefinition[],
        presetOptions: [] as string[],
        health: healthyProfile(selectedProfile.id),
        resolveError: null as Error | null,
        profilesListener: null as (() => void) | null,
        subscribers: { profiles: 0, modelTargets: 0, llmConnections: 0 },
    };

    const profilesApi: TauriTavernAgentProfilesApi = {
        list: () => {
            state.listCount += 1;
            const queued = state.listResults.shift();
            return Promise.resolve(queued ?? {
                profiles: [...state.definitions.values()].map(summary),
                issues: [],
            });
        },
        load: (input) => {
            const profileId = typeof input === 'string' ? input : input.profileId;
            const profile = state.definitions.get(profileId);
            return Promise.resolve({ profile: profile ? structuredClone(profile) : null });
        },
        diagnose: () => Promise.resolve(state.health),
        resolveSystemPrompt: () => state.resolveError
            ? Promise.reject(state.resolveError)
            : Promise.resolve({ agentSystemPrompt: 'Resolved Agent system prompt.' }),
        repairFile: (input) => {
            state.repairs.push(input);
            return Promise.resolve();
        },
        retargetPresetRefs: () => Promise.resolve({ updated: 0, profileIds: [] }),
        save: (input) => {
            const profile = 'profile' in input ? input.profile : input;
            state.saves.push(profile);
            state.definitions.set(profile.id, structuredClone(profile));
            return Promise.resolve();
        },
        delete: (input) => {
            state.definitions.delete(typeof input === 'string' ? input : input.profileId);
            return Promise.resolve();
        },
    };

    const deps: AgentSystemPanelControllerDeps = {
        loadSettings: () => Promise.resolve(state.settings),
        patchSettings: (current, patch) => {
            state.settings = { ...current, ...patch };
            return Promise.resolve(state.settings);
        },
        getProfilesApi: () => profilesApi,
        listTools: () => Promise.resolve({ tools: [], diagnostics: [] }),
        listPresetOptions: () => state.presetOptions,
        listModelTargets: () => [],
        saveModelTargetConnection: () => Promise.resolve(),
        subscribeProfilesChanged: (listener) => {
            state.subscribers.profiles += 1;
            state.profilesListener = listener;
            return () => {
                state.subscribers.profiles -= 1;
            };
        },
        subscribeModelTargetsChanged: () => {
            state.subscribers.modelTargets += 1;
            return () => {
                state.subscribers.modelTargets -= 1;
            };
        },
        subscribeLlmConnectionsChanged: () => {
            state.subscribers.llmConnections += 1;
            return () => {
                state.subscribers.llmConnections -= 1;
            };
        },
        confirmAction: (message) => {
            state.confirmations.push(message);
            return Promise.resolve(state.confirm);
        },
        downloadBlob: () => Promise.resolve({ mode: 'browser-download', completed: true }),
        notifyError: (error) => {
            state.errors.push(error instanceof Error ? error.message : 'unknown error');
        },
        notifyWarning: (message) => {
            state.warnings.push(message);
        },
        notifySuccess: () => undefined,
        onRunsTabActivated: () => undefined,
        tr,
    };
    return { deps, state };
}

const disposables: Array<{ dispose: () => void }> = [];

function renderPanel(controller: AgentSystemPanelController): void {
    const runHistory = createRunHistoryController({
        listRuns: () => Promise.resolve({ runs: [], nextCursor: null }),
        currentChatRunFilter: () => Promise.resolve({
            chatRef: { kind: 'group', chatId: 'group' },
            stableChatId: 'stable-group',
        }),
        openRun: () => undefined,
    });
    const runRetention = createRunRetentionController({
        getRetentionApi: () => ({
            readSettings: () => Promise.resolve({
                autoPruneEnabled: false,
                keepRecentTerminalRuns: 100,
                keepFullRecentRuns: 20,
            }),
            updateSettings: () => Promise.reject(new Error('unused')),
            planPrune: () => Promise.reject(new Error('unused')),
            applyPrune: () => Promise.reject(new Error('unused')),
        }),
        confirmAction: () => Promise.resolve(false),
        notifySuccess: () => undefined,
        notifyWarning: () => undefined,
        tr,
    });
    disposables.push(controller, runHistory, runRetention);
    render(
        <AgentSystemPanelApp
            controller={controller}
            runHistory={runHistory}
            runRetention={runRetention}
            tr={tr}
            onRequestClose={() => undefined}
        />,
    );
}

afterEach(() => {
    cleanup();
    disposables.splice(0).forEach((disposable) => disposable.dispose());
});

test('repairs profile list issues and renders the refreshed list', async () => {
    const { deps, state } = createPanelWorld();
    state.listResults.push(
        {
            profiles: [summary(defaultProfile())],
            issues: [
                {
                    profileId: 'broken-json',
                    fileName: 'broken-json.json',
                    kind: 'invalidJson',
                    recommendedAction: 'delete',
                    message: 'Invalid JSON',
                },
                {
                    profileId: 'bad-schema',
                    fileName: 'bad-schema.json',
                    kind: 'invalidFileIdentity',
                    recommendedAction: 'normalizeIdentity',
                    message: 'Invalid profile kind',
                },
            ],
        },
        {
            profiles: [summary(defaultProfile()), {
                id: 'bad-schema',
                displayName: 'bad-schema',
                directRunnable: true,
            }],
            issues: [],
        },
    );
    const controller = createAgentSystemPanelController(deps);
    renderPanel(controller);

    await act(async () => controller.init());

    expect(state.repairs).toEqual([
        { profileId: 'broken-json', action: 'delete' },
        { profileId: 'bad-schema', action: 'normalizeIdentity' },
    ]);
    expect(state.confirmations).toHaveLength(1);
    expect(state.warnings).toEqual([
        'deletedCorruptAgentProfile id=broken-json',
        'normalizedAgentProfileIdentity id=bad-schema',
    ]);
    expect(screen.getAllByText('bad-schema').length).toBeGreaterThan(0);
});

test('keeps a profile editable when its prompt preview fails', async () => {
    const profile = defaultProfile('dangling-writer');
    profile.displayName = 'Dangling Writer';
    profile.preset = {
        mode: 'ref',
        ref: { apiId: 'openai', name: 'Missing Writer Preset' },
        required: true,
    };
    const { deps, state } = createPanelWorld(profile);
    state.presetOptions = ['Missing Writer Preset'];
    state.resolveError = new Error('agent.profile_preset_missing');
    state.health = {
        profileId: profile.id,
        previewAvailable: true,
        promptAssemblyAvailable: false,
        directRunAvailable: false,
        subAgentAvailable: false,
        diagnostics: [{
            code: 'agent.profile_preset_missing',
            severity: 'error',
            path: '$.preset.ref.name',
            message: 'required preset is missing',
            resource: { kind: 'preset', apiId: 'openai', name: 'Missing Writer Preset' },
            blocks: ['promptAssembly', 'directRun', 'subAgent'],
            repairActions: ['selectPreset'],
        }],
    };
    const controller = createAgentSystemPanelController(deps);
    const user = userEvent.setup();
    renderPanel(controller);
    await act(async () => controller.init());

    await waitFor(() => expect(screen.getByText('agentProfilePresetMissing name=Missing Writer Preset')).toBeDefined());
    const displayName = screen.getByRole<HTMLInputElement>('textbox', { name: 'displayName' });
    await user.clear(displayName);
    await user.type(displayName, 'Editable Writer');

    expect(displayName.value).toBe('Editable Writer');
    expect(controller.getSnapshot().profilePreviewError).toBe('agent.profile_preset_missing');
});

test('supplemental catalog failures do not block profile editing', async () => {
    const profile = defaultProfile('writer');
    const { deps, state } = createPanelWorld(profile);
    deps.listPresetOptions = () => { throw new Error('preset catalog unavailable'); };
    deps.listModelTargets = () => { throw new Error('model catalog unavailable'); };
    deps.listTools = () => Promise.reject(new Error('tool catalog unavailable'));
    const controller = createAgentSystemPanelController(deps);
    const user = userEvent.setup();
    renderPanel(controller);

    await act(async () => controller.init());

    expect(controller.getSnapshot().initialized).toBe(true);
    expect(state.subscribers).toEqual({ profiles: 1, modelTargets: 1, llmConnections: 1 });
    expect(state.errors).toEqual(expect.arrayContaining([
        'preset catalog unavailable',
        'model catalog unavailable',
        'tool catalog unavailable',
    ]));
    const displayName = screen.getByRole<HTMLInputElement>('textbox', { name: 'displayName' });
    await user.clear(displayName);
    await user.type(displayName, 'Still editable');
    expect(displayName.value).toBe('Still editable');
});

test('tool catalog keeps available disabled tools visible', async () => {
    const profile = defaultProfile('writer');
    profile.tools.allow = [];
    const { deps } = createPanelWorld(profile);
    deps.listTools = () => Promise.resolve({
        tools: [{
            id: 'builtin:workspace.read_file',
            nativeName: 'workspace.read_file',
            title: 'Read workspace file',
            description: 'Reads a workspace file.',
            inputSchema: {},
            source: 'builtin',
        }],
        diagnostics: [],
    });
    const controller = createAgentSystemPanelController(deps);
    renderPanel(controller);

    await act(async () => controller.init());

    expect(screen.getAllByText('Read workspace file').length).toBeGreaterThan(0);
});

test('failed profile selection keeps the previous selection and draft together', async () => {
    const writer = defaultProfile('writer');
    const reviewer = defaultProfile('reviewer');
    reviewer.displayName = 'Reviewer';
    const { deps, state } = createPanelWorld(writer);
    state.definitions.set(reviewer.id, reviewer);
    const patchSettings = deps.patchSettings;
    deps.patchSettings = (current, patch) => patch.editingProfileId === reviewer.id
        ? Promise.reject(new Error('settings write failed'))
        : patchSettings(current, patch);
    const controller = createAgentSystemPanelController(deps);
    disposables.push(controller);
    await controller.init();

    await expect(controller.selectProfile(reviewer.id)).rejects.toThrow('settings write failed');

    expect(controller.getSnapshot().editingProfileId).toBe(writer.id);
    expect(controller.getSnapshot().draft.id).toBe(writer.id);
    expect(state.errors).toContain('settings write failed');
});

test('a saved profile remains committed when its list refresh fails', async () => {
    const profile = defaultProfile('writer');
    const { deps, state } = createPanelWorld(profile);
    const controller = createAgentSystemPanelController(deps);
    disposables.push(controller);
    await controller.init();
    deps.getProfilesApi().list = () => Promise.reject(new Error('profile list refresh failed'));
    controller.setIdentityField('displayName', 'Saved Writer');

    await controller.saveProfile();

    expect(state.definitions.get(profile.id)?.displayName).toBe('Saved Writer');
    expect(controller.getSnapshot().draft.displayName).toBe('Saved Writer');
    expect(state.errors).toContain('profile list refresh failed');
});

test('a dirty draft rejects an external overwrite and save', async () => {
    const profile = defaultProfile('writer');
    profile.preset = {
        mode: 'ref',
        ref: { apiId: 'openai', name: 'Old Preset' },
        required: true,
    };
    const { deps, state } = createPanelWorld(profile);
    state.presetOptions = ['Old Preset', 'New Preset'];
    const controller = createAgentSystemPanelController(deps);
    const user = userEvent.setup();
    renderPanel(controller);
    await act(async () => controller.init());

    const displayName = screen.getByRole<HTMLInputElement>('textbox', { name: 'displayName' });
    await user.clear(displayName);
    await user.type(displayName, 'Unsaved local edit');
    const changed = structuredClone(profile);
    if (changed.preset.mode !== 'ref' || !changed.preset.ref) {
        throw new Error('expected a preset reference');
    }
    changed.preset.ref.name = 'New Preset';
    state.definitions.set(profile.id, changed);

    act(() => state.profilesListener?.());
    await waitFor(() => expect(controller.getSnapshot().externalProfileChangePending).toBe(true));
    act(() => state.profilesListener?.());
    await waitFor(() => expect(state.listCount).toBe(3));

    expect(state.warnings).toEqual(['agentProfileExternalChangePending']);
    await expect(controller.saveProfile()).rejects.toThrow('agentProfileExternalChangeSaveBlocked');
    expect(state.saves).toEqual([]);
});

test('dispose and failed subscription setup leave no ghost listeners', async () => {
    const pending = createPanelWorld();
    const deferredSettings: { resolve: ((value: AgentSystemSettings) => void) | null } = { resolve: null };
    pending.deps.loadSettings = () => new Promise((resolve) => {
        deferredSettings.resolve = resolve;
    });
    const pendingController = createAgentSystemPanelController(pending.deps);
    disposables.push(pendingController);
    const initPromise = pendingController.init();
    await waitFor(() => expect(deferredSettings.resolve).not.toBeNull());
    pendingController.dispose();
    deferredSettings.resolve?.(settings('default-writer'));
    await initPromise;

    expect(pending.state.subscribers).toEqual({ profiles: 0, modelTargets: 0, llmConnections: 0 });
    expect(pendingController.getSnapshot().initialized).toBe(false);

    const failed = createPanelWorld();
    failed.deps.subscribeLlmConnectionsChanged = () => {
        throw new Error('LLM subscription failed');
    };
    const failedController = createAgentSystemPanelController(failed.deps);
    disposables.push(failedController);

    await expect(failedController.init()).rejects.toThrow('LLM subscription failed');
    expect(failed.state.subscribers).toEqual({ profiles: 0, modelTargets: 0, llmConnections: 0 });
    expect(failedController.getSnapshot().initialized).toBe(false);
});
