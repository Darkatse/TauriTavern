import { DEFAULT_PROFILE_ID } from './constants.js';
import { errorText, prettyJson } from './host-api.js';
import { loadAgentSystemProfileRuntime } from './AgentSystemProfileRuntime';
import {
    defaultProfile,
    normalizeProfileForSave,
    profileForEdit,
    type AgentProfileDraft,
} from './profile-model';
import {
    rememberMainAgentPresentation,
    type AgentPresentationMemory,
} from './profile-draft-ops';
import {
    PANEL_TABS,
    PROFILE_TOOL_MATRIX_HIDDEN,
    createInitialPanelSnapshot,
    firstProfileSectionIdForMode,
    preferredProfileEditMode,
    profileSectionsForMode,
    type AgentProfileEditMode,
    type AgentProfileSectionId,
    type AgentSystemPanelControllerDeps,
    type AgentSystemPanelSnapshot,
} from './AgentSystemPanelContract';
import { isProfileRuntimeStateCurrent } from './AgentSystemPanelView';
import {
    createAgentSystemDraftEditor,
    type AgentSystemDraftEditor,
    type AgentSystemDraftEditorContext,
} from './AgentSystemDraftEditor';
import {
    createAgentSystemPanelPersistence,
    type AgentSystemPanelPersistence,
} from './AgentSystemPanelPersistence';
import type { AgentSystemSettings } from './AgentSystemEntryController';

export type AgentSystemPanelSession = {
    getSnapshot: () => AgentSystemPanelSnapshot;
    subscribe: (listener: () => void) => () => void;
    init: () => Promise<void>;
    dispose: () => void;
    setActiveProfile: (profileId: string) => Promise<void>;
    setTab: (tab: string) => Promise<void>;
    selectProfile: (profileId: string) => Promise<void>;
    setProfileEditMode: (mode: AgentProfileEditMode) => void;
    scrollToProfileSection: (sectionId: AgentProfileSectionId) => void;
};

export type AgentSystemPanelController = AgentSystemPanelSession
    & AgentSystemDraftEditor
    & Omit<AgentSystemPanelPersistence, 'refreshProfiles'>;

/** Mount-local panel state; dispose blocks every late completion and subscription. */
export function createAgentSystemPanelController(deps: AgentSystemPanelControllerDeps): AgentSystemPanelController {
    let snapshot = createInitialPanelSnapshot();
    const listeners = new Set<() => void>();
    const unsubscribes: Array<() => void> = [];
    let disposed = false;
    let initPromise: Promise<void> | null = null;
    let lastLoadedProfileJson = prettyJson(defaultProfile());
    let profileSelectionToken = 0;
    let profileSelectionSettingsQueue = Promise.resolve();
    // Remember direct-run presentation while converting profiles to/from SubAgent-only.
    const mainAgentPresentationByProfileId: AgentPresentationMemory = {};

    function unsubscribeAll(): void {
        unsubscribes.splice(0).forEach((unsubscribe) => unsubscribe());
    }

    function commit(next: AgentSystemPanelSnapshot): void {
        if (disposed) {
            return;
        }
        snapshot = next;
        for (const listener of listeners) {
            listener();
        }
    }

    function patch(patchValue: Partial<AgentSystemPanelSnapshot>): void {
        commit({ ...snapshot, ...patchValue });
    }

    function reportError(error: unknown): void {
        if (disposed) {
            return;
        }
        const message = errorText(error);
        patch({ error: message });
        deps.notifyError(error);
    }

    async function execute<T>(operation: () => Promise<T>): Promise<T> {
        try {
            return await operation();
        } catch (error) {
            if (snapshot.error !== errorText(error)) {
                reportError(error);
            }
            throw error;
        }
    }

    // Subscription callbacks are fire-and-forget; failures stay visible via
    // the inline error + toastr and surface as unhandled rejections for the
    // dev-log capture.
    function runEventTask(task: () => Promise<void>): void {
        void (async () => {
            try {
                await task();
            } catch (error) {
                reportError(error);
                queueMicrotask(() => {
                    throw error;
                });
            }
        })();
    }

    async function saveSettingsPatch(patchValue: Partial<AgentSystemSettings>): Promise<void> {
        const settings = await deps.patchSettings(snapshot.settings, patchValue);
        patch({ settings });
    }

    function profileExists(profileId: string): boolean {
        return snapshot.profiles.some((profile) => profile.id === profileId);
    }

    function profileIsDirectRunnable(profileId: string): boolean {
        const profile = snapshot.profiles.find((item) => item.id === profileId);
        return Boolean(profile && profile.directRunnable !== false);
    }

    function catalogToolIds(): string[] {
        return snapshot.toolItems.map((tool) => tool.id);
    }

    function toolIdsForDraft(
        draft: AgentProfileDraft,
        toolItems: readonly TauriTavernAgentToolCatalogItem[],
    ): string[] {
        const available = toolItems
            .map((tool) => tool.id)
            .filter((tool) => !PROFILE_TOOL_MATRIX_HIDDEN.has(tool));
        const selected = Array.isArray(draft.tools?.allow) ? draft.tools.allow : [];
        return [...new Set([
            ...available,
            ...selected.filter((tool) => !PROFILE_TOOL_MATRIX_HIDDEN.has(tool)),
        ])];
    }

    function memoryKey(draft: AgentProfileDraft): string {
        const key = (draft.id || snapshot.editingProfileId || DEFAULT_PROFILE_ID).trim() || DEFAULT_PROFILE_ID;
        return key;
    }

    function seedMainAgentPresentation(draft: AgentProfileDraft): void {
        rememberMainAgentPresentation(mainAgentPresentationByProfileId, memoryKey(draft), draft.run.presentation || 'foreground');
    }

    function editModeSyncPatch(draft: AgentProfileDraft): Pick<AgentSystemPanelSnapshot, 'profileEditMode' | 'activeProfileSectionId'> {
        const profileEditMode = preferredProfileEditMode(draft);
        return {
            profileEditMode,
            activeProfileSectionId: firstProfileSectionIdForMode(profileEditMode),
        };
    }

    function clearedRuntimeState(): Pick<AgentSystemPanelSnapshot,
        'resolvedAgentSystemPrompt' | 'profilePreviewError' | 'profileHealth' | 'profileDiagnosticError' | 'profileRuntimeStateJson'> {
        return {
            resolvedAgentSystemPrompt: '',
            profilePreviewError: '',
            profileHealth: null,
            profileDiagnosticError: '',
            profileRuntimeStateJson: '',
        };
    }

    function isCurrentProfileSelection(selectionToken: number, profileId: string | null = null): boolean {
        if (selectionToken !== profileSelectionToken) {
            return false;
        }
        return profileId === null || snapshot.editingProfileId === profileId;
    }

    async function refreshProfileRuntimeState(profileId: string, selectionToken: number): Promise<void> {
        const runtime = await loadAgentSystemProfileRuntime(deps.getProfilesApi(), profileId);
        if (!disposed && isCurrentProfileSelection(selectionToken, profileId)) patch(runtime);
    }

    async function refreshCurrentProfileRuntimeState(): Promise<void> {
        if (!isProfileRuntimeStateCurrent(snapshot.draft, snapshot.profileRuntimeStateJson)) {
            return;
        }
        const profileId = snapshot.editingProfileId || DEFAULT_PROFILE_ID;
        const selectionToken = profileSelectionToken;
        await refreshProfileRuntimeState(profileId, selectionToken);
    }

    async function refreshToolCatalog(): Promise<void> {
        const result = await deps.listTools();
        if (disposed) {
            return;
        }
        const toolIds = toolIdsForDraft(snapshot.draft, result.tools);
        patch({
            toolItems: result.tools,
            toolCatalogDiagnostics: result.diagnostics,
            toolIds,
            selectedToolId: toolIds.includes(snapshot.selectedToolId) ? snapshot.selectedToolId : (toolIds[0] ?? ''),
        });
    }

    function refreshPresetOptions(): void {
        patch({ presetOptions: deps.listPresetOptions() });
    }

    function refreshModelTargets(): void {
        patch({ modelTargets: deps.listModelTargets() });
    }

    async function loadSupplemental(operation: () => void | Promise<void>): Promise<void> {
        try {
            await operation();
        } catch (error) {
            reportError(error);
        }
    }

    async function normalizeProfileSelections(): Promise<void> {
        const activeProfileId = (snapshot.settings.activeProfileId || DEFAULT_PROFILE_ID).trim() || DEFAULT_PROFILE_ID;
        const editingProfileId = (snapshot.settings.editingProfileId || activeProfileId).trim() || activeProfileId;
        const patchValue: Partial<AgentSystemSettings> = {};
        const activeProfileNeedsReset = !profileIsDirectRunnable(activeProfileId);
        if (activeProfileNeedsReset) {
            patchValue.activeProfileId = DEFAULT_PROFILE_ID;
        }
        if (!profileExists(editingProfileId)) {
            patchValue.editingProfileId = DEFAULT_PROFILE_ID;
        }
        if (Object.keys(patchValue).length > 0) {
            await saveSettingsPatch(patchValue);
        }
        if (activeProfileNeedsReset && activeProfileId !== DEFAULT_PROFILE_ID) {
            deps.notifyWarning(deps.tr('activeProfileResetToDefault'));
        }
    }

    async function selectProfile(profileId: string, options: { persistEditing?: boolean } = {}): Promise<void> {
        const id = profileId || DEFAULT_PROFILE_ID;
        const selectionToken = ++profileSelectionToken;
        const result = await deps.getProfilesApi().load({ profileId: id });
        if (disposed || !isCurrentProfileSelection(selectionToken)) {
            return;
        }
        if (!result?.profile) {
            throw new Error(deps.tr('agentProfileNotFound', { id }));
        }
        const loadedJson = prettyJson(normalizeProfileForSave(result.profile));
        const draft = profileForEdit(result.profile);
        seedMainAgentPresentation(draft);
        if (options.persistEditing !== false) {
            const persist: Promise<void> = profileSelectionSettingsQueue.then(async () => {
                if (disposed || !isCurrentProfileSelection(selectionToken)) {
                    return;
                }
                await saveSettingsPatch({ editingProfileId: id });
            });
            profileSelectionSettingsQueue = persist.then<void, void>(() => undefined, () => undefined);
            await persist;
            if (disposed || !isCurrentProfileSelection(selectionToken)) {
                return;
            }
        }
        lastLoadedProfileJson = loadedJson;
        patch({
            editingProfileId: id,
            externalProfileChangePending: false,
            draft,
            toolIds: toolIdsForDraft(draft, snapshot.toolItems),
            ...editModeSyncPatch(draft),
            draftJson: prettyJson(normalizeProfileForSave(draft)),
            ...clearedRuntimeState(),
            profileRuntimeStateJson: loadedJson,
        });
        void refreshProfileRuntimeState(id, selectionToken);
    }

    function currentProfileDraftHasUnsavedChanges(): boolean {
        return prettyJson(normalizeProfileForSave(snapshot.draft)) !== lastLoadedProfileJson;
    }

    async function handleProfilesChanged(): Promise<void> {
        if (!snapshot.initialized || snapshot.saving) {
            return;
        }
        await persistence.refreshProfiles({ repairListIssues: false });
        if (disposed) {
            return;
        }
        const profileId = snapshot.editingProfileId || DEFAULT_PROFILE_ID;
        const result = await deps.getProfilesApi().load({ profileId });
        if (disposed) {
            return;
        }
        const loadedProfileJson = result?.profile
            ? prettyJson(normalizeProfileForSave(result.profile))
            : null;
        if (currentProfileDraftHasUnsavedChanges()) {
            if (lastLoadedProfileJson && loadedProfileJson !== lastLoadedProfileJson) {
                if (!snapshot.externalProfileChangePending) {
                    deps.notifyWarning(deps.tr('agentProfileExternalChangePending'));
                }
                patch({ externalProfileChangePending: true });
            }
            return;
        }
        if (!result?.profile) {
            await selectProfile(DEFAULT_PROFILE_ID, { persistEditing: false });
            return;
        }
        await selectProfile(profileId, { persistEditing: false });
    }

    async function handleModelTargetsChanged(): Promise<void> {
        if (!snapshot.initialized) {
            return;
        }
        refreshModelTargets();
        if (disposed || snapshot.saving) {
            return;
        }
        await refreshCurrentProfileRuntimeState();
    }

    async function handleLlmConnectionsChanged(): Promise<void> {
        if (!snapshot.initialized || snapshot.saving) {
            return;
        }
        await refreshCurrentProfileRuntimeState();
    }

    const persistence = createAgentSystemPanelPersistence({
        deps,
        getSnapshot: () => snapshot,
        commit: patch,
        isDisposed: () => disposed,
        saveSettingsPatch,
        selectProfile,
        reportError,
    });

    const draftEditorContext: AgentSystemDraftEditorContext = {
        deps,
        getSnapshot: () => snapshot,
        commit: patch,
        isDisposed: () => disposed,
        presentationMemory: mainAgentPresentationByProfileId,
        memoryKey,
        seedMainAgentPresentation,
        editModeSyncPatch,
        toolIdsForDraft: draft => toolIdsForDraft(draft, snapshot.toolItems),
        catalogToolIds,
        clearedRuntimeState,
        bumpProfileSelectionToken: () => {
            profileSelectionToken += 1;
        },
        resetLastLoadedProfileJson: () => {
            lastLoadedProfileJson = '';
        },
    };
    const draftEditor = createAgentSystemDraftEditor(draftEditorContext);

    async function init(): Promise<void> {
        initPromise ??= (async () => {
            patch({ loading: true });
            try {
                let settings = await deps.loadSettings();
                if (disposed) {
                    return;
                }
                if (!PANEL_TABS.some((tab) => tab.id === settings.activeTab)) {
                    settings = await deps.patchSettings(settings, { activeTab: 'profiles' });
                    if (disposed) {
                        return;
                    }
                }
                patch({ settings });
                await persistence.refreshProfiles();
                if (disposed) {
                    return;
                }
                await normalizeProfileSelections();
                if (disposed) {
                    return;
                }
                const editingProfileId = snapshot.settings.editingProfileId || DEFAULT_PROFILE_ID;
                await selectProfile(editingProfileId, { persistEditing: false });
                if (disposed) {
                    return;
                }
                // Fix 4: Host subscriptions install only after a successful
                // init; a popup closed mid-init never registers listeners.
                unsubscribes.push(deps.subscribeProfilesChanged(() => {
                    runEventTask(handleProfilesChanged);
                }));
                unsubscribes.push(deps.subscribeModelTargetsChanged(() => {
                    runEventTask(handleModelTargetsChanged);
                }));
                unsubscribes.push(deps.subscribeLlmConnectionsChanged(() => {
                    runEventTask(handleLlmConnectionsChanged);
                }));
                patch({ initialized: true });
                if (snapshot.settings.activeTab === 'runs') {
                    deps.onRunsTabActivated();
                }
                await Promise.all([
                    loadSupplemental(refreshPresetOptions),
                    loadSupplemental(refreshModelTargets),
                    loadSupplemental(refreshToolCatalog),
                ]);
            } catch (error) {
                unsubscribeAll();
                reportError(error);
                throw error;
            } finally {
                if (!disposed) {
                    patch({ loading: false });
                }
            }
        })();
        return initPromise;
    }

    function dispose(): void {
        if (disposed) {
            return;
        }
        disposed = true;
        unsubscribeAll();
        listeners.clear();
    }

    return {
        getSnapshot: () => snapshot,
        subscribe(listener) {
            listeners.add(listener);
            return () => {
                listeners.delete(listener);
            };
        },
        init,
        dispose,
        setActiveProfile(profileId: string): Promise<void> {
            return execute(async () => {
                const id = profileId.trim();
                if (!profileExists(id)) {
                    throw new Error(deps.tr('agentProfileNotFound', { id }));
                }
                if (!profileIsDirectRunnable(id)) {
                    throw new Error(deps.tr('agentProfileNotDirectRunnable', { id }));
                }
                await saveSettingsPatch({ activeProfileId: id });
            });
        },
        setTab(tab: string): Promise<void> {
            return execute(async () => {
                const previousTab = snapshot.settings.activeTab;
                await saveSettingsPatch({ activeTab: tab });
                if (disposed) {
                    return;
                }
                if (tab === 'runs' && previousTab !== 'runs') {
                    deps.onRunsTabActivated();
                }
            });
        },
        selectProfile: (profileId: string) => execute(() => selectProfile(profileId)),
        setProfileEditMode(mode: AgentProfileEditMode): void {
            patch({ profileEditMode: mode, activeProfileSectionId: firstProfileSectionIdForMode(mode) });
        },
        scrollToProfileSection(sectionId: AgentProfileSectionId): void {
            if (!profileSectionsForMode(snapshot.profileEditMode).some((section) => section.id === sectionId)) {
                throw new Error(`Unknown Agent profile section: ${sectionId}`);
            }
            patch({
                activeProfileSectionId: sectionId,
                profileSectionScrollRequest: snapshot.profileSectionScrollRequest + 1,
            });
        },
        ...draftEditor,
        saveProfile: () => execute(persistence.saveProfile),
        deleteProfile: () => execute(persistence.deleteProfile),
        exportSelectedProfile: () => execute(persistence.exportSelectedProfile),
    };
}
