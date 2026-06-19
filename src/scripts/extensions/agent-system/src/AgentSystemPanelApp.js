import {
    AGENT_DELEGATION_TOOLS,
    DEFAULT_PROFILE_ID,
    KNOWN_TOOLS,
    RUNTIME_ONLY_TOOLS,
    WORKSPACE_ROOTS,
} from './constants.js';
import { confirmAction, errorText, prettyJson, requireAgentApi, requireSillyTavernContext } from './host-api.js';
import { translateAgentSystem as tr } from './i18n.js';
import {
    findModelTargetForBinding,
    listSavedModelTargets,
    modelBindingFromTarget,
    saveModelTargetAsLlmConnection,
    modelTargetIdFromConnectionRef,
    subscribeModelTargetChanges,
} from './model-target-connection.js';
import {
    defaultProfile,
    normalizeDelegationToolAllowList,
    normalizeProfileForSave,
    normalizeProfileId,
    profileForEdit,
} from './profile-model.js';
import { RunHistoryPanel } from './RunHistoryPanel.js';
import { loadSettings, patchSettings } from './settings-store.js';
import { downloadBlobWithRuntime } from '../../../file-export.js';
import { subscribeAgentProfilesChanged } from '../../../tauritavern/agent/agent-profile-events.js';
import { AGENT_MODEL_REQUIRES_CONFIGURATION, sanitizePortableAgentProfile } from '../../../tauritavern/agent/agent-profile-portable.js';
import { normalizeAgentSystemPrompt } from '../../../tauritavern/agent/agent-system-prompt.js';
import { subscribeLlmConnectionsChanged } from '../../../tauritavern/agent/llm-connection-events.js';

const PROFILE_EXPORT_CONTENT_TYPE = 'application/json';
const CHAT_COMPLETION_PRESET_API_ID = 'openai';
const PROFILE_DIAGNOSTIC_CODES = Object.freeze({
    PROFILE_CONTRACT_INVALID: 'agent.profile_contract_invalid',
    PRESET_API_UNSUPPORTED: 'agent.profile_preset_api_unsupported',
    PRESET_MISSING: 'agent.profile_preset_missing',
    MODEL_REQUIRES_CONFIGURATION: 'agent.profile_model_requires_configuration',
    MODEL_CONNECTION_MISSING: 'agent.profile_model_connection_missing',
    MODEL_CONNECTION_INVALID: 'agent.profile_model_connection_invalid',
});
const PROFILE_TOOL_MATRIX_HIDDEN = new Set([
    ...AGENT_DELEGATION_TOOLS,
    ...RUNTIME_ONLY_TOOLS,
]);

const PROFILE_EDIT_MODES = Object.freeze([
    { id: 'main', labelKey: 'mainAgent', icon: 'fa-compass-drafting' },
    { id: 'subagent', labelKey: 'subAgent', icon: 'fa-people-arrows' },
]);

const PROFILE_SECTIONS = Object.freeze([
    { id: 'identity', labelKey: 'identity', icon: 'fa-fingerprint', modes: ['main', 'subagent'] },
    { id: 'binding', labelKey: 'presetAndModel', icon: 'fa-sliders', modes: ['main', 'subagent'] },
    { id: 'main-delegation', labelKey: 'mainAgentControl', icon: 'fa-diagram-project', modes: ['main'] },
    { id: 'subagent-access', labelKey: 'subAgentAccess', icon: 'fa-people-arrows', modes: ['subagent'] },
    { id: 'run', labelKey: 'runPolicy', icon: 'fa-gauge-high', modes: ['main', 'subagent'] },
    { id: 'context', labelKey: 'initialContext', icon: 'fa-layer-group', modes: ['main', 'subagent'] },
    { id: 'prompt', labelKey: 'prompt', icon: 'fa-terminal', modes: ['main', 'subagent'] },
    { id: 'tools', labelKey: 'capabilityMatrix', icon: 'fa-screwdriver-wrench', modes: ['main', 'subagent'] },
    { id: 'skills', labelKey: 'skillAccess', icon: 'fa-book', modes: ['main', 'subagent'] },
    { id: 'workspace', labelKey: 'workspaceAccess', icon: 'fa-folder-tree', modes: ['main', 'subagent'] },
    { id: 'output', labelKey: 'outputArtifact', icon: 'fa-file-lines', modes: ['main'] },
    { id: 'json', labelKey: 'advancedJson', icon: 'fa-code', modes: ['main', 'subagent'] },
]);

function isProfileEditMode(mode) {
    return PROFILE_EDIT_MODES.some((item) => item.id === mode);
}

function firstProfileSectionIdForMode(mode) {
    const section = PROFILE_SECTIONS.find((item) => item.modes.includes(mode));
    if (!section) {
        throw new Error(`Unsupported Agent profile edit mode: ${mode}`);
    }
    return section.id;
}

function preferredProfileEditMode(profile) {
    return profile?.run?.directRunnable === false ? 'subagent' : 'main';
}

const TOOL_GROUPS = Object.freeze([
    {
        id: 'context',
        labelKey: 'contextTools',
        icon: 'fa-comments',
        tools: ['chat.search', 'chat.read_messages', 'worldinfo.read_activated'],
    },
    {
        id: 'skills',
        labelKey: 'skillTools',
        icon: 'fa-book-open',
        tools: ['skill.list', 'skill.search', 'skill.read'],
    },
    {
        id: 'workspace-read',
        labelKey: 'workspaceReadTools',
        icon: 'fa-folder-tree',
        tools: ['workspace.list_files', 'workspace.search_files', 'workspace.read_file'],
    },
    {
        id: 'workspace-write',
        labelKey: 'workspaceWriteTools',
        icon: 'fa-pen-to-square',
        tools: ['workspace.write_file', 'workspace.apply_patch'],
    },
    {
        id: 'control',
        labelKey: 'controlTools',
        icon: 'fa-flag-checkered',
        tools: ['workspace.commit', 'workspace.finish'],
    },
    {
        id: 'other',
        labelKey: 'otherTools',
        icon: 'fa-dice',
        tools: ['dice.roll'],
    },
]);

const WORKSPACE_ROOT_ICONS = Object.freeze({
    output: 'fa-message',
    scratch: 'fa-note-sticky',
    plan: 'fa-list-check',
    summaries: 'fa-layer-group',
    persist: 'fa-database',
});

function normalizeResolvedAgentSystemPrompt(result) {
    return normalizeAgentSystemPrompt(result?.agentSystemPrompt);
}

function profileDiagnosticMessage(diagnostic) {
    const resource = diagnostic?.resource || {};
    switch (diagnostic?.code) {
        case PROFILE_DIAGNOSTIC_CODES.PRESET_MISSING:
            return tr('agentProfilePresetMissing', { name: resource.name || diagnostic.path });
        case PROFILE_DIAGNOSTIC_CODES.PRESET_API_UNSUPPORTED:
            return tr('agentProfilePresetUnsupported', { apiId: resource.apiId || '' });
        case PROFILE_DIAGNOSTIC_CODES.MODEL_REQUIRES_CONFIGURATION:
            return tr('modelRequiresConfiguration');
        case PROFILE_DIAGNOSTIC_CODES.MODEL_CONNECTION_MISSING:
            return tr('agentProfileModelBindingMissing', { id: resource.id || '' });
        case PROFILE_DIAGNOSTIC_CODES.MODEL_CONNECTION_INVALID:
            return tr('agentProfileModelBindingInvalid', { id: resource.id || '' });
        case PROFILE_DIAGNOSTIC_CODES.PROFILE_CONTRACT_INVALID:
            return tr('agentProfileContractInvalid', { error: diagnostic.message || diagnostic.code });
        default:
            return diagnostic?.message || diagnostic?.code || tr('unknownError');
    }
}

function uniqueMessages(messages) {
    return [...new Set(messages.filter(Boolean))];
}

export function createAgentSystemPanelRoot({ requestClose }) {
    return {
        components: {
            RunHistoryPanel,
        },
        data() {
            return {
                initialized: false,
                loading: false,
                saving: false,
                error: '',
                unsubscribeProfilesChanged: null,
                unsubscribeModelTargetsChanged: null,
                unsubscribeLlmConnectionsChanged: null,
                externalProfileChangePending: false,
                settings: {},
                profiles: [],
                editingProfileId: DEFAULT_PROFILE_ID,
                // UI-only editor view. The saved profile role is owned by run/delegation policy.
                profileEditMode: 'main',
                activeProfileSectionId: firstProfileSectionIdForMode('main'),
                // Remember direct-run presentation while converting profiles to/from SubAgent-only.
                mainAgentPresentationByProfileId: {},
                draft: profileForEdit(defaultProfile()),
                draftJson: prettyJson(defaultProfile()),
                lastLoadedProfileJson: prettyJson(defaultProfile()),
                resolvedAgentSystemPrompt: '',
                profilePreviewError: '',
                profileHealth: null,
                profileDiagnosticError: '',
                profileRuntimeStateJson: '',
                profileSelectionToken: 0,
                tabs: [
                    { id: 'profiles', labelKey: 'profiles', icon: 'fa-id-card-clip' },
                    { id: 'runs', labelKey: 'runs', icon: 'fa-clock-rotate-left' },
                ],
                toolSpecs: [],
                toolNames: [...KNOWN_TOOLS],
                selectedToolName: KNOWN_TOOLS[0],
                workspaceRoots: WORKSPACE_ROOTS,
                presetOptions: [],
                modelTargets: [],
            };
        },
        computed: {
            activeTab() {
                return this.settings.activeTab;
            },
            activeProfileId() {
                return this.settings.activeProfileId || DEFAULT_PROFILE_ID;
            },
            activeProfileOptions() {
                return this.profiles.filter((profile) => profile.directRunnable !== false);
            },
            isBuiltinProfile() {
                return this.draft.id === DEFAULT_PROFILE_ID;
            },
            profileEditModes() {
                return PROFILE_EDIT_MODES;
            },
            visibleProfileSections() {
                return PROFILE_SECTIONS.filter((section) => section.modes.includes(this.profileEditMode));
            },
            profileEditModeLabel() {
                const mode = PROFILE_EDIT_MODES.find((item) => item.id === this.profileEditMode);
                return mode ? tr(mode.labelKey) : tr('mainAgent');
            },
            isSubAgentPresentationLocked() {
                return this.isSubAgentOnly;
            },
            isCallableAsSubAgent() {
                return Boolean(this.draft?.delegation?.callable && this.draft?.delegation?.allowAsSubagent);
            },
            isCallableAsHandoffTarget() {
                return Boolean(this.draft?.delegation?.callable && this.draft?.delegation?.allowAsHandoffTarget);
            },
            isSubAgentOnly() {
                return this.draft?.run?.directRunnable === false;
            },
            agentSystemPromptEditorValue() {
                if (this.isBuiltinProfile) {
                    return this.resolvedAgentSystemPrompt;
                }
                return this.draft.instructions.agentSystemPrompt ?? '';
            },
            agentSystemPromptPlaceholder() {
                if (this.isBuiltinProfile || String(this.draft.instructions.agentSystemPrompt || '').trim()) {
                    return '';
                }
                return this.isProfileRuntimeStateCurrent ? this.resolvedAgentSystemPrompt : '';
            },
            isProfileRuntimeStateCurrent() {
                return Boolean(this.profileRuntimeStateJson)
                    && prettyJson(normalizeProfileForSave(this.draft)) === this.profileRuntimeStateJson;
            },
            profileStats() {
                const allowedTools = new Set(Array.isArray(this.draft?.tools?.allow) ? this.draft.tools.allow : []);
                const enabledToolCount = this.toolNames.filter((tool) => allowedTools.has(tool)).length;
                const visibleRootCount = Array.isArray(this.draft?.workspace?.visibleRoots)
                    ? this.draft.workspace.visibleRoots.length
                    : 0;
                const writableRootCount = Array.isArray(this.draft?.workspace?.writableRoots)
                    ? this.draft.workspace.writableRoots.length
                    : 0;
                return [
                    {
                        icon: 'fa-scroll',
                        label: tr('preset'),
                        value: this.presetSummaryLabel,
                    },
                    {
                        icon: 'fa-microchip',
                        label: tr('model'),
                        value: this.modelSummaryLabel,
                    },
                    {
                        icon: this.profileEditMode === 'subagent' ? 'fa-people-arrows' : 'fa-compass-drafting',
                        label: tr('profileView'),
                        value: this.profileEditModeLabel,
                    },
                    {
                        icon: 'fa-layer-group',
                        label: tr('presentation'),
                        value: tr(this.draft.run.presentation || 'foreground'),
                    },
                    {
                        icon: 'fa-diagram-project',
                        label: tr('agentCooperation'),
                        value: this.delegationSummaryLabel,
                    },
                    {
                        icon: 'fa-screwdriver-wrench',
                        label: tr('tools'),
                        value: `${enabledToolCount}/${this.toolNames.length}`,
                    },
                    {
                        icon: 'fa-folder-tree',
                        label: tr('workspaceRoots'),
                        value: `${writableRootCount}/${visibleRootCount}`,
                    },
                ];
            },
            presetSummaryLabel() {
                const preset = this.draft?.preset || {};
                if (preset.mode === 'ref') {
                    return preset.ref?.name || tr('savedPreset');
                }
                if (preset.mode === 'none') {
                    return tr('none');
                }
                return tr('currentPromptPreset');
            },
            delegationSummaryLabel() {
                const delegation = this.draft?.delegation || {};
                if (this.profileEditMode === 'subagent') {
                    return delegation.callable && delegation.allowAsSubagent ? tr('callableSubAgent') : tr('notCallable');
                }
                const summary = [];
                if (delegation.canDelegate && delegation.canHandoff) {
                    summary.push(tr('canDelegateAndHandoff'));
                } else if (delegation.canHandoff) {
                    summary.push(tr('canHandoff'));
                } else if (delegation.canDelegate) {
                    summary.push(tr('canDelegate'));
                }
                if (delegation.callable && delegation.allowAsHandoffTarget) {
                    summary.push(tr('handoffTarget'));
                }
                return summary.length > 0 ? summary.join(' / ') : tr('delegationOff');
            },
            availablePresetOptions() {
                const names = [...this.presetOptions];
                const selected = this.draft?.preset?.mode === 'ref' ? String(this.draft.preset.ref?.name || '').trim() : '';
                if (selected && !names.includes(selected)) {
                    names.push(selected);
                }
                return names;
            },
            modelSummaryLabel() {
                const model = this.draft?.model || {};
                if (model.mode === AGENT_MODEL_REQUIRES_CONFIGURATION) {
                    return tr('modelRequiresConfiguration');
                }
                if (model.mode !== 'connectionRef') {
                    return tr('currentChatModel');
                }
                const target = this.selectedModelTarget;
                if (target) {
                    return target.name || target.model;
                }
                return model.modelId || model.connectionRef || tr('savedModel');
            },
            selectedModelTarget() {
                return findModelTargetForBinding(this.modelTargets, this.draft?.model);
            },
            selectedModelTargetId() {
                return this.selectedModelTarget?.id || '';
            },
            hasExternalModelBinding() {
                return this.draft?.model?.mode === 'connectionRef' && !this.selectedModelTarget;
            },
            missingPresetName() {
                const preset = this.draft?.preset || {};
                if (preset.mode !== 'ref') {
                    return '';
                }
                const name = String(preset.ref?.name || '').trim();
                if (!name || this.presetOptions.includes(name)) {
                    return '';
                }
                return name;
            },
            profileDiagnostics() {
                if (!this.isProfileRuntimeStateCurrent || !this.profileHealth) {
                    return [];
                }
                return Array.isArray(this.profileHealth.diagnostics) ? this.profileHealth.diagnostics : [];
            },
            profileConfigurationWarnings() {
                const warnings = [];
                const diagnostics = this.profileDiagnostics;
                if (diagnostics.length > 0) {
                    warnings.push(...diagnostics.map(profileDiagnosticMessage));
                } else {
                    if (this.missingPresetName) {
                        warnings.push(tr('agentProfilePresetMissing', { name: this.missingPresetName }));
                    }
                    if (this.hasExternalModelBinding) {
                        warnings.push(tr('agentProfileModelBindingMissing', { id: this.draft.model.connectionRef }));
                    }
                }
                if (this.isProfileRuntimeStateCurrent && this.profileDiagnosticError) {
                    warnings.push(tr('agentProfileDiagnosticsUnavailable', { error: this.profileDiagnosticError }));
                }
                const diagnosticCoversPreview = diagnostics.some((diagnostic) => (
                    Array.isArray(diagnostic.blocks) && diagnostic.blocks.includes('preview')
                ));
                if (this.isProfileRuntimeStateCurrent && this.profilePreviewError && !diagnosticCoversPreview) {
                    warnings.push(tr('agentProfilePreviewUnavailable', { error: this.profilePreviewError }));
                }
                return uniqueMessages(warnings);
            },
            toolGroupsWithTools() {
                const groupedTools = new Set();
                const groups = TOOL_GROUPS
                    .map((group) => {
                        const tools = group.tools.filter((tool) => this.toolNames.includes(tool));
                        tools.forEach((tool) => groupedTools.add(tool));
                        return { ...group, tools };
                    })
                    .filter((group) => group.tools.length > 0);
                const extraTools = this.toolNames.filter((tool) => !groupedTools.has(tool));
                if (extraTools.length > 0) {
                    groups.push({
                        id: 'extra',
                        labelKey: 'otherTools',
                        icon: 'fa-ellipsis',
                        tools: extraTools,
                    });
                }
                return groups;
            },
            toolSpecsByName() {
                return Object.fromEntries(this.toolSpecs.map((spec) => [spec.name, spec]));
            },
            selectedToolSpec() {
                return this.toolSpecsByName[this.selectedToolName] || null;
            },
            selectedToolEnabled() {
                return Array.isArray(this.draft?.tools?.allow) && this.draft.tools.allow.includes(this.selectedToolName);
            },
            selectedToolProperties() {
                const properties = this.selectedToolSpec?.inputSchema?.properties || {};
                const required = new Set(Array.isArray(this.selectedToolSpec?.inputSchema?.required)
                    ? this.selectedToolSpec.inputSchema.required
                    : []);
                return Object.entries(properties).map(([name, schema]) => ({
                    name,
                    schema,
                    required: required.has(name),
                    type: this.schemaType(schema),
                    description: String(schema?.description || ''),
                }));
            },
        },
        async mounted() {
            await this.initialize();
            this.unsubscribeProfilesChanged = subscribeAgentProfilesChanged(() => {
                void this.handleProfilesChanged();
            });
            this.unsubscribeModelTargetsChanged = subscribeModelTargetChanges(() => {
                void this.handleModelTargetsChanged();
            });
            this.unsubscribeLlmConnectionsChanged = subscribeLlmConnectionsChanged(() => {
                void this.handleLlmConnectionsChanged();
            });
        },
        unmounted() {
            this.unsubscribeProfilesChanged?.();
            this.unsubscribeModelTargetsChanged?.();
            this.unsubscribeLlmConnectionsChanged?.();
        },
        methods: {
            async initialize() {
                this.loading = true;
                try {
                    this.settings = await loadSettings();
                    if (!this.tabs.some((tab) => tab.id === this.settings.activeTab)) {
                        this.settings = await patchSettings(this.settings, { activeTab: 'profiles' });
                    }
                    await Promise.all([
                        this.refreshToolSpecs(),
                        this.refreshProfiles(),
                        this.refreshPresetOptions(),
                        this.refreshModelTargets(),
                    ]);
                    await this.normalizeProfileSelections();
                    this.editingProfileId = this.settings.editingProfileId || DEFAULT_PROFILE_ID;
                    await this.selectProfile(this.editingProfileId, { persistEditing: false });
                    this.initialized = true;
                } catch (error) {
                    this.reportError(error);
                    throw error;
                } finally {
                    this.loading = false;
                }
            },
            closePanel() {
                requestClose();
            },
            tr(key, params) {
                return tr(key, params);
            },
            async saveSettingsPatch(patch) {
                this.settings = await patchSettings(this.settings, patch);
            },
            profileExists(profileId) {
                return this.profiles.some((profile) => profile.id === profileId);
            },
            profileIsDirectRunnable(profileId) {
                const profile = this.profiles.find((item) => item.id === profileId);
                return Boolean(profile && profile.directRunnable !== false);
            },
            async normalizeProfileSelections() {
                const activeProfileId = String(this.settings.activeProfileId || DEFAULT_PROFILE_ID).trim() || DEFAULT_PROFILE_ID;
                const editingProfileId = String(this.settings.editingProfileId || activeProfileId).trim() || activeProfileId;
                const patch = {};
                const activeProfileNeedsReset = !this.profileIsDirectRunnable(activeProfileId);
                if (activeProfileNeedsReset) {
                    patch.activeProfileId = DEFAULT_PROFILE_ID;
                }
                if (!this.profileExists(editingProfileId)) {
                    patch.editingProfileId = DEFAULT_PROFILE_ID;
                }
                if (Object.keys(patch).length > 0) {
                    await this.saveSettingsPatch(patch);
                }
                if (activeProfileNeedsReset && activeProfileId !== DEFAULT_PROFILE_ID) {
                    this.warn(tr('activeProfileResetToDefault'));
                }
            },
            async setActiveProfile(profileId) {
                const id = String(profileId || '').trim();
                if (!this.profileExists(id)) {
                    throw new Error(tr('agentProfileNotFound', { id }));
                }
                if (!this.profileIsDirectRunnable(id)) {
                    throw new Error(tr('agentProfileNotDirectRunnable', { id }));
                }
                await this.saveSettingsPatch({ activeProfileId: id });
            },
            async setTab(tab) {
                await this.saveSettingsPatch({ activeTab: tab });
            },
            setProfileEditMode(mode) {
                if (!isProfileEditMode(mode)) {
                    throw new Error(`Unsupported Agent profile edit mode: ${mode}`);
                }
                this.profileEditMode = mode;
                this.resetActiveProfileSection();
            },
            syncProfileEditModeToDraft() {
                this.profileEditMode = preferredProfileEditMode(this.draft);
                this.resetActiveProfileSection();
            },
            resetActiveProfileSection() {
                this.activeProfileSectionId = firstProfileSectionIdForMode(this.profileEditMode);
            },
            profilePresentationMemoryKey() {
                return String(this.draft?.id || this.editingProfileId || DEFAULT_PROFILE_ID).trim() || DEFAULT_PROFILE_ID;
            },
            rememberMainAgentPresentation() {
                this.mainAgentPresentationByProfileId[this.profilePresentationMemoryKey()] = this.draft.run.presentation || 'foreground';
            },
            seedMainAgentPresentation() {
                this.rememberMainAgentPresentation();
            },
            restoreMainAgentPresentation() {
                this.draft.run.presentation = this.mainAgentPresentationByProfileId[this.profilePresentationMemoryKey()] || 'foreground';
            },
            applySubAgentOnlyRunPolicy() {
                if (!this.isCallableAsSubAgent) {
                    throw new Error('SubAgent-only run policy requires callable SubAgent delegation.');
                }
                if (!Object.prototype.hasOwnProperty.call(this.mainAgentPresentationByProfileId, this.profilePresentationMemoryKey())) {
                    this.seedMainAgentPresentation();
                }
                // Callable SubAgent profiles enter through TaskReturnRequired child invocations, not direct foreground chat runs.
                this.draft.run.directRunnable = false;
                this.draft.run.presentation = 'background';
            },
            scrollToProfileSection(sectionId) {
                if (!this.visibleProfileSections.some((section) => section.id === sectionId)) {
                    throw new Error(`Unknown Agent profile section: ${sectionId}`);
                }
                this.activeProfileSectionId = sectionId;
                this.$nextTick(() => {
                    const section = this.$el?.querySelector?.(`[data-ttas-profile-section="${sectionId}"]`);
                    section?.scrollIntoView?.({ behavior: 'smooth', block: 'start' });
                });
            },
            async refreshProfiles(options = {}) {
                const profilesApi = requireAgentApi().profiles;
                const result = await profilesApi.list();
                this.profiles = Array.isArray(result?.profiles) ? result.profiles : [];
                const issues = Array.isArray(result?.issues) ? result.issues : [];
                if (options.repairListIssues === false || issues.length === 0) {
                    return;
                }
                const repaired = await this.repairProfileListIssues(issues);
                if (repaired) {
                    const refreshed = await profilesApi.list();
                    this.profiles = Array.isArray(refreshed?.profiles) ? refreshed.profiles : [];
                }
            },
            async repairProfileListIssues(issues) {
                const profilesApi = requireAgentApi().profiles;
                if (typeof profilesApi.repairFile !== 'function') {
                    throw new Error(tr('hostAgentProfileApiUnavailable'));
                }

                let repaired = false;
                for (const issue of issues) {
                    const profileId = String(issue?.profileId || '').trim();
                    const action = String(issue?.recommendedAction || '').trim();
                    if (!profileId) {
                        throw new Error('Agent profile repair issue is missing profileId');
                    }
                    if (!action) {
                        this.warn(tr('agentProfileManualRepairRequired', {
                            id: profileId,
                            error: String(issue?.message || ''),
                        }));
                        continue;
                    }
                    if (action === 'delete') {
                        const message = tr('deleteCorruptAgentProfileConfirm', {
                            id: profileId,
                            error: String(issue?.message || ''),
                        });
                        if (!await confirmAction(message)) {
                            continue;
                        }
                        try {
                            await profilesApi.repairFile({ profileId, action });
                        } catch (error) {
                            this.reportError(error);
                            continue;
                        }
                        this.warn(tr('deletedCorruptAgentProfile', { id: profileId }));
                        repaired = true;
                        continue;
                    }
                    if (action === 'normalizeIdentity') {
                        try {
                            await profilesApi.repairFile({ profileId, action });
                        } catch (error) {
                            this.reportError(error);
                            continue;
                        }
                        this.warn(tr('normalizedAgentProfileIdentity', { id: profileId }));
                        repaired = true;
                        continue;
                    }
                    throw new Error(`Unsupported Agent profile repair action: ${action}`);
                }
                return repaired;
            },
            async handleProfilesChanged() {
                if (!this.initialized || this.saving) {
                    return;
                }
                try {
                    await this.refreshProfiles({ repairListIssues: false });
                    const profileId = this.editingProfileId || DEFAULT_PROFILE_ID;
                    const result = await requireAgentApi().profiles.load({ profileId });
                    const loadedProfileJson = result?.profile
                        ? prettyJson(normalizeProfileForSave(result.profile))
                        : null;
                    if (this.currentProfileDraftHasUnsavedChanges()) {
                        if (this.lastLoadedProfileJson && loadedProfileJson !== this.lastLoadedProfileJson) {
                            if (!this.externalProfileChangePending) {
                                this.warn(tr('agentProfileExternalChangePending'));
                            }
                            this.externalProfileChangePending = true;
                        }
                        return;
                    }
                    if (!result?.profile) {
                        await this.selectProfile(DEFAULT_PROFILE_ID, { persistEditing: false });
                        return;
                    }
                    await this.selectProfile(profileId, { persistEditing: false });
                } catch (error) {
                    this.reportError(error);
                    queueMicrotask(() => {
                        throw error;
                    });
                }
            },
            async handleModelTargetsChanged() {
                if (!this.initialized) {
                    return;
                }
                try {
                    await this.refreshModelTargets();
                    if (!this.saving) {
                        await this.refreshCurrentProfileRuntimeState();
                    }
                } catch (error) {
                    this.reportError(error);
                    queueMicrotask(() => {
                        throw error;
                    });
                }
            },
            async handleLlmConnectionsChanged() {
                if (!this.initialized || this.saving) {
                    return;
                }
                try {
                    await this.refreshCurrentProfileRuntimeState();
                } catch (error) {
                    this.reportError(error);
                    queueMicrotask(() => {
                        throw error;
                    });
                }
            },
            async refreshCurrentProfileRuntimeState() {
                if (!this.isProfileRuntimeStateCurrent) {
                    return;
                }
                const profileId = this.editingProfileId || DEFAULT_PROFILE_ID;
                const selectionToken = this.profileSelectionToken;
                await this.refreshProfileRuntimeState(requireAgentApi().profiles, profileId, selectionToken);
            },
            async refreshToolSpecs() {
                const api = requireAgentApi().tools;
                if (!api?.list) {
                    throw new Error(tr('hostAgentToolApiUnavailable'));
                }
                const result = await api.list();
                this.toolSpecs = result.tools;
                this.toolNames = this.toolSpecs
                    .map((tool) => tool.name)
                    .filter((tool) => !PROFILE_TOOL_MATRIX_HIDDEN.has(tool));
                if (!this.toolNames.includes(this.selectedToolName)) {
                    this.selectedToolName = this.toolNames[0];
                }
            },
            async refreshPresetOptions() {
                const manager = requireSillyTavernContext().getPresetManager?.(CHAT_COMPLETION_PRESET_API_ID);
                if (!manager) {
                    throw new Error(tr('presetManagerUnavailable'));
                }
                this.presetOptions = manager
                    .getAllPresets()
                    .map((name) => String(name || '').trim())
                    .filter((name) => name && manager.findPreset(name) !== 'gui')
                    .sort((a, b) => a.localeCompare(b));
            },
            async refreshModelTargets() {
                this.modelTargets = listSavedModelTargets();
            },
            async selectProfile(profileId, options = {}) {
                const id = profileId || DEFAULT_PROFILE_ID;
                const selectionToken = ++this.profileSelectionToken;
                const profilesApi = requireAgentApi().profiles;
                const result = await profilesApi.load({ profileId: id });
                if (!this.isCurrentProfileSelection(selectionToken)) {
                    return;
                }
                if (!result?.profile) {
                    throw new Error(tr('agentProfileNotFound', { id }));
                }
                this.editingProfileId = id;
                if (options.persistEditing !== false) {
                    await this.saveSettingsPatch({ editingProfileId: id });
                    if (!this.isCurrentProfileSelection(selectionToken, id)) {
                        return;
                    }
                }
                this.lastLoadedProfileJson = prettyJson(normalizeProfileForSave(result.profile));
                this.externalProfileChangePending = false;
                this.draft = profileForEdit(result.profile);
                this.seedMainAgentPresentation();
                this.syncProfileEditModeToDraft();
                this.refreshDraftJson();
                this.clearProfileRuntimeState();
                this.profileRuntimeStateJson = this.lastLoadedProfileJson;
                await this.refreshProfileRuntimeState(profilesApi, id, selectionToken);
            },
            isCurrentProfileSelection(selectionToken, profileId = null) {
                if (selectionToken !== this.profileSelectionToken) {
                    return false;
                }
                return profileId == null || this.editingProfileId === profileId;
            },
            clearProfileRuntimeState() {
                this.resolvedAgentSystemPrompt = '';
                this.profilePreviewError = '';
                this.profileHealth = null;
                this.profileDiagnosticError = '';
                this.profileRuntimeStateJson = '';
            },
            invalidateProfileRuntimeState() {
                this.profileSelectionToken += 1;
                this.clearProfileRuntimeState();
            },
            async refreshProfileRuntimeState(profilesApi, profileId, selectionToken) {
                await this.refreshProfileHealth(profilesApi, profileId, selectionToken);
                await this.refreshProfilePreview(profilesApi, profileId, selectionToken);
            },
            async refreshProfileHealth(profilesApi, profileId, selectionToken) {
                try {
                    const health = await profilesApi.diagnose({ profileId });
                    if (!this.isCurrentProfileSelection(selectionToken, profileId)) {
                        return;
                    }
                    this.profileHealth = health || null;
                    this.profileDiagnosticError = '';
                } catch (error) {
                    if (!this.isCurrentProfileSelection(selectionToken, profileId)) {
                        return;
                    }
                    this.profileHealth = null;
                    this.profileDiagnosticError = errorText(error);
                }
            },
            async refreshProfilePreview(profilesApi, profileId, selectionToken) {
                try {
                    const promptResult = await profilesApi.resolveSystemPrompt({ profileId });
                    if (!this.isCurrentProfileSelection(selectionToken, profileId)) {
                        return;
                    }
                    this.resolvedAgentSystemPrompt = normalizeResolvedAgentSystemPrompt(promptResult);
                    this.profilePreviewError = '';
                } catch (error) {
                    if (!this.isCurrentProfileSelection(selectionToken, profileId)) {
                        return;
                    }
                    this.resolvedAgentSystemPrompt = '';
                    this.profilePreviewError = errorText(error);
                }
            },
            refreshDraftJson() {
                this.draftJson = prettyJson(normalizeProfileForSave(this.draft));
            },
            applyDraftJson() {
                const parsed = JSON.parse(this.draftJson);
                this.draft = profileForEdit(parsed);
                this.editingProfileId = parsed.id;
                this.seedMainAgentPresentation();
                this.syncProfileEditModeToDraft();
                this.invalidateProfileRuntimeState();
            },
            newProfile() {
                const id = this.nextProfileId('agent-profile');
                this.editingProfileId = id;
                this.draft = profileForEdit(defaultProfile(id));
                this.lastLoadedProfileJson = '';
                this.externalProfileChangePending = false;
                this.seedMainAgentPresentation();
                this.syncProfileEditModeToDraft();
                this.invalidateProfileRuntimeState();
                this.refreshDraftJson();
            },
            copyProfile() {
                const id = this.nextProfileId(`${this.draft.id}-copy`);
                const copy = normalizeProfileForSave(this.draft);
                copy.id = id;
                copy.displayName = tr('copyDisplayName', { name: copy.displayName });
                this.editingProfileId = id;
                this.draft = profileForEdit(copy);
                this.lastLoadedProfileJson = '';
                this.externalProfileChangePending = false;
                this.seedMainAgentPresentation();
                this.syncProfileEditModeToDraft();
                this.invalidateProfileRuntimeState();
                this.refreshDraftJson();
            },
            setAgentSystemPromptDraft(event) {
                if (this.isBuiltinProfile) {
                    return;
                }
                this.draft.instructions.agentSystemPrompt = event.target.value;
                this.invalidateProfileRuntimeState();
            },
            setPresetMode(mode) {
                if (this.isBuiltinProfile) {
                    return;
                }
                if (mode === 'currentPromptSnapshot') {
                    this.draft.preset = {
                        mode: 'currentPromptSnapshot',
                        required: false,
                    };
                    this.invalidateProfileRuntimeState();
                    return;
                }
                if (mode === 'none') {
                    this.draft.preset = {
                        mode: 'none',
                        required: false,
                    };
                    this.invalidateProfileRuntimeState();
                    return;
                }
                if (mode !== 'ref') {
                    throw new Error(`Unsupported preset mode: ${mode}`);
                }

                const name = String(this.draft.preset?.ref?.name || this.presetOptions[0] || '').trim();
                this.draft.preset = {
                    mode: 'ref',
                    ref: {
                        apiId: CHAT_COMPLETION_PRESET_API_ID,
                        name,
                    },
                    required: true,
                };
                this.invalidateProfileRuntimeState();
            },
            setPresetName(name) {
                if (this.isBuiltinProfile) {
                    return;
                }
                this.draft.preset = {
                    mode: 'ref',
                    ref: {
                        apiId: CHAT_COMPLETION_PRESET_API_ID,
                        name: String(name || '').trim(),
                    },
                    required: true,
                };
                this.invalidateProfileRuntimeState();
            },
            setModelMode(mode) {
                if (this.isBuiltinProfile) {
                    return;
                }
                if (mode === 'currentPromptSnapshot') {
                    this.draft.model = {
                        mode: 'currentPromptSnapshot',
                    };
                    this.invalidateProfileRuntimeState();
                    return;
                }
                if (mode === AGENT_MODEL_REQUIRES_CONFIGURATION) {
                    this.draft.model = {
                        mode: AGENT_MODEL_REQUIRES_CONFIGURATION,
                    };
                    this.invalidateProfileRuntimeState();
                    return;
                }
                if (mode !== 'connectionRef') {
                    throw new Error(`Unsupported model mode: ${mode}`);
                }
                if (this.draft.model?.mode === 'connectionRef') {
                    return;
                }
                const target = this.modelTargets[0];
                if (!target) {
                    throw new Error(tr('noSavedModelTargets'));
                }
                this.draft.model = modelBindingFromTarget(target);
                this.invalidateProfileRuntimeState();
            },
            setModelTarget(targetId) {
                if (this.isBuiltinProfile) {
                    return;
                }
                const target = this.modelTargets.find((item) => item.id === targetId);
                if (!target) {
                    throw new Error(tr('savedModelTargetNotFound', { id: targetId }));
                }
                this.draft.model = modelBindingFromTarget(target);
                this.invalidateProfileRuntimeState();
            },
            modelTargetBadges(target) {
                const badges = [
                    String(target.api || '').trim(),
                    String(target['custom-api-format'] || '').trim(),
                    String(target.model || '').trim(),
                ].filter(Boolean);
                if (target['api-url']) {
                    badges.push(String(target['api-url']).trim());
                }
                return badges;
            },
            setCanDelegate(enabled) {
                if (this.isBuiltinProfile) {
                    return;
                }
                this.draft.delegation.canDelegate = Boolean(enabled);
                this.syncDelegationTools();
            },
            setCanHandoff(enabled) {
                if (this.isBuiltinProfile) {
                    return;
                }
                this.draft.delegation.canHandoff = Boolean(enabled);
                this.syncDelegationTools();
            },
            setRunPresentation(presentation) {
                if (this.isBuiltinProfile) {
                    return;
                }
                if (this.isSubAgentPresentationLocked) {
                    throw new Error('SubAgent-only profiles are locked to background presentation.');
                }
                if (presentation !== 'foreground' && presentation !== 'background') {
                    throw new Error(`Unsupported Agent run presentation: ${presentation}`);
                }
                this.draft.run.presentation = presentation;
                this.rememberMainAgentPresentation();
            },
            setCallableAsSubAgent(enabled) {
                if (this.isBuiltinProfile) {
                    return;
                }
                const isEnabled = Boolean(enabled);
                this.draft.delegation.allowAsSubagent = isEnabled;
                this.draft.delegation.callable = isEnabled || Boolean(this.draft.delegation.allowAsHandoffTarget);
                if (isEnabled) {
                    this.applySubAgentOnlyRunPolicy();
                    this.syncProfileEditModeToDraft();
                    return;
                }
                if (this.isSubAgentOnly) {
                    this.draft.run.directRunnable = true;
                    this.restoreMainAgentPresentation();
                }
                this.syncProfileEditModeToDraft();
            },
            setCallableAsHandoffTarget(enabled) {
                if (this.isBuiltinProfile) {
                    return;
                }
                const isEnabled = Boolean(enabled);
                this.draft.delegation.allowAsHandoffTarget = isEnabled;
                this.draft.delegation.callable = isEnabled || Boolean(this.draft.delegation.allowAsSubagent);
                if (!isEnabled && !this.draft.delegation.allowAsSubagent && this.isSubAgentOnly) {
                    this.draft.run.directRunnable = true;
                    this.restoreMainAgentPresentation();
                    this.syncProfileEditModeToDraft();
                }
            },
            syncDelegationTools() {
                this.draft.tools.allow = normalizeDelegationToolAllowList(
                    this.draft?.tools?.allow,
                    this.draft?.delegation,
                    this.toolSpecs.map((tool) => tool.name),
                );
            },
            async persistProfileModelBinding(profile) {
                if (profile?.model?.mode !== 'connectionRef' || !modelTargetIdFromConnectionRef(profile.model.connectionRef)) {
                    return;
                }
                const modelTargets = listSavedModelTargets();
                this.modelTargets = modelTargets;
                const target = findModelTargetForBinding(modelTargets, profile.model);
                if (!target) {
                    return;
                }
                await saveModelTargetAsLlmConnection(target);
            },
            nextProfileId(base) {
                const normalized = normalizeProfileId(base) || 'agent-profile';
                const ids = new Set(this.profiles.map((profile) => profile.id));
                if (!ids.has(normalized)) {
                    return normalized;
                }
                for (let index = 2; index < 1000; index += 1) {
                    const candidate = `${normalized}-${index}`;
                    if (!ids.has(candidate)) {
                        return candidate;
                    }
                }
                throw new Error(tr('unableToAllocateProfileId'));
            },
            syncWritableRoots() {
                const visible = new Set(this.draft.workspace.visibleRoots);
                this.draft.workspace.writableRoots = this.draft.workspace.writableRoots.filter((root) => visible.has(root));
            },
            enabledToolCount(tools) {
                const allow = new Set(Array.isArray(this.draft.tools.allow) ? this.draft.tools.allow : []);
                return tools.filter((tool) => allow.has(tool)).length;
            },
            selectTool(toolName) {
                this.selectedToolName = toolName;
            },
            toolSpec(toolName) {
                return this.toolSpecsByName[toolName];
            },
            toolTitle(toolName) {
                return this.toolSpec(toolName)?.title || toolName;
            },
            toolModelName(toolName) {
                return this.toolSpec(toolName)?.modelName || toolName.replace(/\./g, '_');
            },
            toolSource(toolName) {
                return this.toolSpec(toolName)?.source || '';
            },
            schemaType(schema) {
                const type = schema?.type;
                if (Array.isArray(type)) {
                    return type.join(' | ');
                }
                return String(type || tr('value'));
            },
            toolBadges(toolName) {
                const spec = this.toolSpec(toolName);
                const annotations = spec?.annotations || {};
                const badges = [];
                if (annotations.readOnly) {
                    badges.push({ key: 'read', label: tr('readOnlyTool') });
                }
                if (annotations.mutating) {
                    badges.push({ key: 'write', label: tr('mutatingTool') });
                }
                if (annotations.control) {
                    badges.push({ key: 'control', label: tr('controlTool') });
                }
                if (this.toolHasDescriptionOverride(toolName)) {
                    badges.push({ key: 'custom', label: tr('customizedTool') });
                }
                return badges;
            },
            toolHasDescriptionOverride(toolName) {
                const override = this.draft?.tools?.toolDescriptions?.[toolName];
                return Boolean(override?.description || Object.keys(override?.properties || {}).length > 0);
            },
            getToolDescriptionOverride(toolName) {
                return this.draft.tools.toolDescriptions?.[toolName]?.description || '';
            },
            getToolPropertyDescriptionOverride(toolName, property) {
                return this.draft.tools.toolDescriptions?.[toolName]?.properties?.[property] || '';
            },
            setToolDescriptionOverride(toolName, value) {
                this.updateToolDescriptionOverride(toolName, (override) => {
                    const description = String(value || '');
                    if (description.trim()) {
                        override.description = description;
                    } else {
                        delete override.description;
                    }
                });
            },
            setToolPropertyDescriptionOverride(toolName, property, value) {
                this.updateToolDescriptionOverride(toolName, (override) => {
                    const description = String(value || '');
                    const properties = { ...(override.properties || {}) };
                    if (description.trim()) {
                        properties[property] = description;
                    } else {
                        delete properties[property];
                    }
                    if (Object.keys(properties).length > 0) {
                        override.properties = properties;
                    } else {
                        delete override.properties;
                    }
                });
            },
            updateToolDescriptionOverride(toolName, mutate) {
                const toolDescriptions = { ...(this.draft.tools.toolDescriptions || {}) };
                const override = { ...(toolDescriptions[toolName] || {}) };
                mutate(override);
                if (!override.description && !override.properties) {
                    delete toolDescriptions[toolName];
                } else {
                    toolDescriptions[toolName] = override;
                }
                this.draft.tools.toolDescriptions = toolDescriptions;
            },
            resetToolDescriptionOverride(toolName) {
                const toolDescriptions = { ...(this.draft.tools.toolDescriptions || {}) };
                delete toolDescriptions[toolName];
                this.draft.tools.toolDescriptions = toolDescriptions;
            },
            resetToolPropertyDescriptionOverride(toolName, property) {
                this.updateToolDescriptionOverride(toolName, (override) => {
                    const properties = { ...(override.properties || {}) };
                    delete properties[property];
                    if (Object.keys(properties).length > 0) {
                        override.properties = properties;
                    } else {
                        delete override.properties;
                    }
                });
            },
            async toggleToolAllowed(toolName, event) {
                const enabled = event.target.checked;
                const allow = new Set(this.draft.tools.allow);
                if (enabled) {
                    allow.add(toolName);
                } else {
                    if (this.toolHasDescriptionOverride(toolName)) {
                        if (!await confirmAction(tr('removeToolDescriptionOnDisableConfirm', { tool: toolName }))) {
                            event.target.checked = true;
                            return;
                        }
                        this.resetToolDescriptionOverride(toolName);
                    }
                    allow.delete(toolName);
                }
                const hiddenAllowed = this.draft.tools.allow
                    .filter((tool) => PROFILE_TOOL_MATRIX_HIDDEN.has(tool) && !RUNTIME_ONLY_TOOLS.includes(tool));
                this.draft.tools.allow = [
                    ...hiddenAllowed,
                    ...this.toolNames.filter((tool) => allow.has(tool)),
                ];
            },
            workspaceRootIcon(root) {
                return WORKSPACE_ROOT_ICONS[root] || 'fa-folder';
            },
            async saveProfile() {
                if (this.isBuiltinProfile) {
                    throw new Error(tr('agentProfileBuiltInEdit'));
                }
                if (this.externalProfileChangePending) {
                    throw new Error(tr('agentProfileExternalChangeSaveBlocked'));
                }
                this.saving = true;
                try {
                    const profile = normalizeProfileForSave(this.draft);
                    const wasActiveProfile = this.activeProfileId === profile.id;
                    await this.persistProfileModelBinding(profile);
                    await requireAgentApi().profiles.save({ profile });
                    await this.refreshProfiles();
                    const settingsPatch = { editingProfileId: profile.id };
                    if (profile.run.directRunnable === false && wasActiveProfile) {
                        settingsPatch.activeProfileId = DEFAULT_PROFILE_ID;
                    }
                    await this.saveSettingsPatch(settingsPatch);
                    await this.selectProfile(profile.id, { persistEditing: false });
                    if (settingsPatch.activeProfileId) {
                        this.warn(tr('activeProfileResetToDefault'));
                    }
                    this.toast(tr('agentProfileSaved'));
                } catch (error) {
                    this.reportError(error);
                    throw error;
                } finally {
                    this.saving = false;
                }
            },
            profileDraftHasUnsavedChanges(savedProfile) {
                return prettyJson(normalizeProfileForSave(this.draft)) !== prettyJson(savedProfile);
            },
            currentProfileDraftHasUnsavedChanges() {
                return prettyJson(normalizeProfileForSave(this.draft)) !== this.lastLoadedProfileJson;
            },
            async exportSelectedProfile() {
                const profileId = this.editingProfileId || DEFAULT_PROFILE_ID;
                const result = await requireAgentApi().profiles.load({ profileId });
                const profile = result?.profile;
                if (!profile) {
                    throw new Error(tr('agentProfileNotFound', { id: profileId }));
                }
                if (profileId !== DEFAULT_PROFILE_ID && this.profileDraftHasUnsavedChanges(profile)) {
                    throw new Error(tr('agentProfileExportSaveFirst'));
                }

                const portableProfile = sanitizePortableAgentProfile(profile);
                const blob = new Blob([`${prettyJson(portableProfile)}\n`], { type: PROFILE_EXPORT_CONTENT_TYPE });
                const downloadResult = await downloadBlobWithRuntime(blob, `${profile.id}.agent-profile.json`, {
                    fallbackName: 'agent-profile.json',
                });
                if (downloadResult?.mode !== 'ios-native-share' || downloadResult.completed === true) {
                    this.toast(tr('exportedProfile', { id: profile.id }));
                }
            },
            async deleteProfile() {
                if (this.isBuiltinProfile) {
                    throw new Error(tr('agentProfileBuiltInDelete'));
                }
                const id = this.draft.id;
                if (!await confirmAction(tr('deleteAgentProfileConfirm', { id }))) {
                    return;
                }
                await requireAgentApi().profiles.delete({ profileId: id });
                await this.refreshProfiles();
                await this.saveSettingsPatch({
                    editingProfileId: DEFAULT_PROFILE_ID,
                    ...(this.activeProfileId === id ? { activeProfileId: DEFAULT_PROFILE_ID } : {}),
                });
                await this.selectProfile(DEFAULT_PROFILE_ID, { persistEditing: false });
                this.toast(tr('deletedProfile', { id }));
            },
            prettyJson(value) {
                return prettyJson(value);
            },
            reportError(error) {
                const message = errorText(error);
                this.error = message;
                console.error('[AgentSystem]', error);
                toastr.error(message);
            },
            toast(message) {
                toastr.success(message);
            },
            warn(message) {
                toastr.warning(message);
            },
        },
        template: `
            <div class="ttas-root ttas-panel-root">
                <header class="ttas-titlebar">
                    <div class="ttas-titlebar-main">
                        <div class="ttas-title-icon" aria-hidden="true">
                            <i class="fa-solid fa-atom"></i>
                        </div>
                        <div class="ttas-title-copy">
                            <div class="ttas-eyebrow">{{ tr('tauriTavernAgent') }}</div>
                            <h3>{{ tr('agentSystem') }}</h3>
                        </div>
                    </div>
                    <button type="button" class="menu_button menu_button_icon ttas-close-button" :title="tr('close')" @click="closePanel">
                        <i class="fa-solid fa-xmark"></i>
                    </button>
                </header>

                <div v-if="loading && !initialized" class="ttas-loading">{{ tr('loadingAgentSystem') }}</div>
                <div v-else class="ttas-panel-body">
                    <div v-if="error" class="ttas-error">
                        <i class="fa-solid fa-triangle-exclamation"></i>
                        <pre>{{ error }}</pre>
                    </div>

                    <nav class="ttas-tabs">
                        <button v-for="tab in tabs" :key="tab.id" type="button" class="menu_button" :class="{ active: activeTab === tab.id }" @click="setTab(tab.id)">
                            <i class="fa-solid" :class="tab.icon"></i>
                            <span>{{ tr(tab.labelKey) }}</span>
                        </button>
                    </nav>

                    <transition name="ttas-panel-fade" mode="out-in">
                    <section v-if="activeTab === 'profiles'" key="profiles" class="ttas-panel">
                        <div class="ttas-profile-layout">
                            <aside class="ttas-list ttas-side-list">
                                <div class="ttas-list-header">
                                    <h4>{{ tr('profiles') }}</h4>
                                    <span>{{ tr('profileCount', { count: profiles.length }) }}</span>
                                </div>
                                <label class="ttas-field ttas-side-active-profile">
                                    <span>{{ tr('activeProfile') }}</span>
                                    <select :value="activeProfileId" @change="setActiveProfile($event.target.value)">
                                        <option v-for="profile in activeProfileOptions" :key="profile.id" :value="profile.id">{{ profile.displayName || profile.id }}</option>
                                    </select>
                                </label>
                                <button
                                    v-for="profile in profiles"
                                    :key="profile.id"
                                    type="button"
                                    :class="{ active: editingProfileId === profile.id, 'is-run-profile': activeProfileId === profile.id }"
                                    @click="selectProfile(profile.id)"
                                >
                                    <strong>{{ profile.displayName }}</strong>
                                    <span>
                                        {{ profile.id }}
                                        <em v-if="activeProfileId === profile.id" class="ttas-active-profile-badge">{{ tr('activeProfileShort') }}</em>
                                    </span>
                                    <small v-if="profile.description">{{ profile.description }}</small>
                                </button>
                            </aside>
                            <nav class="ttas-section-rail" :aria-label="tr('profileSections')">
                                <button
                                    v-for="section in visibleProfileSections"
                                    :key="section.id"
                                    type="button"
                                    class="ttas-section-jump"
                                    :class="{ active: activeProfileSectionId === section.id }"
                                    :title="tr(section.labelKey)"
                                    @click="scrollToProfileSection(section.id)"
                                >
                                    <i class="fa-solid" :class="section.icon"></i>
                                    <span>{{ tr(section.labelKey) }}</span>
                                </button>
                            </nav>
                            <div class="ttas-editor">
                                <div class="ttas-mobile-profile-controls">
                                    <label class="ttas-field">
                                        <span>{{ tr('editingProfile') }}</span>
                                        <select :value="editingProfileId" @change="selectProfile($event.target.value)">
                                            <option v-for="profile in profiles" :key="profile.id" :value="profile.id">{{ profile.displayName }}</option>
                                        </select>
                                    </label>
                                    <label class="ttas-field">
                                        <span>{{ tr('activeProfile') }}</span>
                                        <select :value="activeProfileId" @change="setActiveProfile($event.target.value)">
                                            <option v-for="profile in activeProfileOptions" :key="profile.id" :value="profile.id">{{ profile.displayName || profile.id }}</option>
                                        </select>
                                    </label>
                                </div>

                                <div class="ttas-editor-hero">
                                    <div class="ttas-hero-copy">
                                        <div class="ttas-eyebrow">{{ tr('profileSummary') }}</div>
                                        <h4>{{ draft.displayName || draft.id }}</h4>
                                        <p>{{ draft.id }}</p>
                                    </div>
                                    <div class="ttas-editor-actions">
                                        <button type="button" class="menu_button menu_button_icon" @click="newProfile">
                                            <i class="fa-solid fa-plus"></i>
                                            <span>{{ tr('new') }}</span>
                                        </button>
                                        <button type="button" class="menu_button menu_button_icon" @click="copyProfile">
                                            <i class="fa-solid fa-copy"></i>
                                            <span>{{ tr('copy') }}</span>
                                        </button>
                                        <button type="button" class="menu_button menu_button_icon" @click="exportSelectedProfile" :disabled="saving">
                                            <i class="fa-solid fa-file-export"></i>
                                            <span>{{ tr('export') }}</span>
                                        </button>
                                        <button type="button" class="menu_button menu_button_icon ttas-primary-button" @click="saveProfile" :disabled="saving || isBuiltinProfile">
                                            <i class="fa-solid" :class="saving ? 'fa-spinner fa-spin' : 'fa-floppy-disk'"></i>
                                            <span>{{ tr('save') }}</span>
                                        </button>
                                        <button type="button" class="menu_button menu_button_icon ttas-danger-button" @click="deleteProfile" :disabled="isBuiltinProfile">
                                            <i class="fa-solid fa-trash-can"></i>
                                            <span>{{ tr('delete') }}</span>
                                        </button>
                                    </div>
                                    <div class="ttas-profile-mode-switch" :aria-label="tr('profileView')">
                                        <button
                                            v-for="mode in profileEditModes"
                                            :key="mode.id"
                                            type="button"
                                            class="menu_button menu_button_icon"
                                            :class="{ active: profileEditMode === mode.id }"
                                            @click="setProfileEditMode(mode.id)"
                                        >
                                            <i class="fa-solid" :class="mode.icon"></i>
                                            <span>{{ tr(mode.labelKey) }}</span>
                                        </button>
                                    </div>
                                    <div class="ttas-stat-grid">
                                        <div v-for="stat in profileStats" :key="stat.label" class="ttas-stat">
                                            <i class="fa-solid" :class="stat.icon"></i>
                                            <span>{{ stat.label }}</span>
                                            <strong>{{ stat.value }}</strong>
                                        </div>
                                    </div>
                                </div>

                                <div class="ttas-section" data-ttas-profile-section="identity">
                                    <div class="ttas-section-title">
                                        <i class="fa-solid fa-fingerprint"></i>
                                        <h4>{{ tr('identity') }}</h4>
                                    </div>
                                    <div class="ttas-form-grid">
                                        <label class="ttas-field">
                                            <span>{{ tr('profileId') }}</span>
                                            <input class="text_pole" v-model="draft.id" :disabled="isBuiltinProfile" />
                                        </label>
                                        <label class="ttas-field">
                                            <span>{{ tr('displayName') }}</span>
                                            <input class="text_pole" v-model="draft.displayName" :disabled="isBuiltinProfile" />
                                        </label>
                                        <label class="ttas-field ttas-span-2">
                                            <span>{{ tr('description') }}</span>
                                            <input class="text_pole" v-model="draft.description" :disabled="isBuiltinProfile" />
                                        </label>
                                    </div>
                                </div>

                                <div class="ttas-section" data-ttas-profile-section="binding">
                                    <div class="ttas-section-title">
                                        <i class="fa-solid fa-sliders"></i>
                                        <h4>{{ tr('presetAndModel') }}</h4>
                                    </div>
                                    <div class="ttas-form-grid">
                                        <label class="ttas-field">
                                            <span>{{ tr('presetSource') }}</span>
                                            <select :value="draft.preset.mode" :disabled="isBuiltinProfile" @change="setPresetMode($event.target.value)">
                                                <option value="currentPromptSnapshot">{{ tr('currentPromptPreset') }}</option>
                                                <option value="ref">{{ tr('savedChatCompletionPreset') }}</option>
                                                <option value="none">{{ tr('noPromptPreset') }}</option>
                                            </select>
                                        </label>
                                        <label v-if="draft.preset.mode === 'ref'" class="ttas-field">
                                            <span>{{ tr('savedPreset') }}</span>
                                            <select :value="draft.preset.ref?.name || ''" :disabled="isBuiltinProfile" @change="setPresetName($event.target.value)">
                                                <option v-if="availablePresetOptions.length === 0" value="">{{ tr('none') }}</option>
                                                <option v-for="name in availablePresetOptions" :key="name" :value="name">{{ name }}</option>
                                            </select>
                                        </label>
                                        <div v-else class="ttas-binding-status">
                                            <i class="fa-solid fa-scroll"></i>
                                            <strong>{{ presetSummaryLabel }}</strong>
                                            <span>{{ tr('preset') }}</span>
                                        </div>

                                        <label class="ttas-field">
                                            <span>{{ tr('modelSource') }}</span>
                                            <select :value="draft.model.mode" :disabled="isBuiltinProfile" @change="setModelMode($event.target.value)">
                                                <option value="currentPromptSnapshot">{{ tr('currentChatModel') }}</option>
                                                <option v-if="draft.model.mode === 'requiresConfiguration'" value="requiresConfiguration">{{ tr('modelRequiresConfiguration') }}</option>
                                                <option value="connectionRef" :disabled="modelTargets.length === 0 && draft.model.mode !== 'connectionRef'">{{ tr('savedModelTarget') }}</option>
                                            </select>
                                        </label>
                                        <label v-if="draft.model.mode === 'connectionRef' && modelTargets.length > 0" class="ttas-field">
                                            <span>{{ tr('savedModel') }}</span>
                                            <select :value="selectedModelTargetId" :disabled="isBuiltinProfile" @change="setModelTarget($event.target.value)">
                                                <option v-if="hasExternalModelBinding" value="">{{ modelSummaryLabel }}</option>
                                                <option v-for="target in modelTargets" :key="target.id" :value="target.id">{{ target.name || target.model }}</option>
                                            </select>
                                        </label>
                                        <div v-else class="ttas-binding-status">
                                            <i class="fa-solid fa-microchip"></i>
                                            <strong>{{ modelSummaryLabel }}</strong>
                                            <span>{{ tr('model') }}</span>
                                        </div>

                                        <div v-if="selectedModelTarget" class="ttas-binding-summary ttas-span-2">
                                            <i class="fa-solid fa-plug-circle-check"></i>
                                            <div>
                                                <strong>{{ selectedModelTarget.name || selectedModelTarget.model }}</strong>
                                                <div class="ttas-tool-badge-row">
                                                    <span v-for="badge in modelTargetBadges(selectedModelTarget)" :key="badge">{{ badge }}</span>
                                                </div>
                                            </div>
                                        </div>
                                        <div v-else-if="hasExternalModelBinding" class="ttas-binding-summary ttas-binding-warning ttas-span-2">
                                            <i class="fa-solid fa-link"></i>
                                            <div>
                                                <strong>{{ draft.model.connectionRef }}</strong>
                                                <span>{{ draft.model.modelId }}</span>
                                            </div>
                                        </div>
                                        <div v-if="profileConfigurationWarnings.length > 0" class="ttas-binding-summary ttas-binding-warning ttas-span-2">
                                            <i class="fa-solid fa-triangle-exclamation"></i>
                                            <div>
                                                <strong>{{ tr('agentProfileNeedsRepair') }}</strong>
                                                <span v-for="warning in profileConfigurationWarnings" :key="warning">{{ warning }}</span>
                                            </div>
                                        </div>
                                    </div>
                                </div>

                                <div v-if="profileEditMode === 'main'" class="ttas-section" data-ttas-profile-section="main-delegation">
                                    <div class="ttas-section-title">
                                        <i class="fa-solid fa-diagram-project"></i>
                                        <h4>{{ tr('mainAgentControl') }}</h4>
                                    </div>
                                    <div class="ttas-delegation-panel">
                                        <label class="ttas-switch-row">
                                            <input type="checkbox" :checked="draft.delegation.canDelegate" :disabled="isBuiltinProfile" @change="setCanDelegate($event.target.checked)" />
                                            <span>
                                                <strong>{{ tr('delegateToSubAgents') }}</strong>
                                                <small>{{ tr('delegateToSubAgentsHint') }}</small>
                                            </span>
                                        </label>
                                        <div v-if="draft.delegation.canDelegate" class="ttas-form-grid ttas-delegation-controls">
                                            <label class="ttas-field">
                                                <span>{{ tr('maxConcurrentSubAgents') }}</span>
                                                <input class="text_pole" type="number" min="1" v-model.number="draft.delegation.maxConcurrentInvocations" :disabled="isBuiltinProfile" />
                                            </label>
                                            <label class="ttas-field">
                                                <span>{{ tr('maxSubAgentTasks') }}</span>
                                                <input class="text_pole" type="number" min="1" v-model.number="draft.delegation.maxInvocationsPerRun" :disabled="isBuiltinProfile" />
                                            </label>
                                        </div>
                                        <label class="ttas-switch-row">
                                            <input type="checkbox" :checked="draft.delegation.canHandoff" :disabled="isBuiltinProfile" @change="setCanHandoff($event.target.checked)" />
                                            <span>
                                                <strong>{{ tr('allowAgentHandoff') }}</strong>
                                                <small>{{ tr('allowAgentHandoffHint') }}</small>
                                            </span>
                                        </label>
                                        <div v-if="draft.delegation.canHandoff" class="ttas-form-grid ttas-delegation-controls">
                                            <label class="ttas-field">
                                                <span>{{ tr('maxHandoffDepth') }}</span>
                                                <input class="text_pole" type="number" min="1" v-model.number="draft.delegation.maxHandoffDepth" :disabled="isBuiltinProfile" />
                                            </label>
                                        </div>
                                        <label class="ttas-switch-row">
                                            <input type="checkbox" :checked="isCallableAsHandoffTarget" :disabled="isBuiltinProfile" @change="setCallableAsHandoffTarget($event.target.checked)" />
                                            <span>
                                                <strong>{{ tr('callableHandoffTargetToggle') }}</strong>
                                                <small>{{ tr('callableHandoffTargetHint') }}</small>
                                            </span>
                                        </label>
                                        <div v-if="isCallableAsHandoffTarget" class="ttas-form-grid ttas-delegation-controls">
                                            <label class="ttas-field ttas-span-2">
                                                <span>{{ tr('agentFacingDescription') }}</span>
                                                <textarea class="text_pole textarea_compact" rows="4" v-model="draft.delegation.descriptionForAgents" :disabled="isBuiltinProfile" :placeholder="draft.description"></textarea>
                                                <small class="ttas-field-hint">
                                                    <i class="fa-solid fa-circle-info" aria-hidden="true"></i>
                                                    <span>{{ tr('agentFacingDescriptionHint') }}</span>
                                                </small>
                                            </label>
                                            <label class="ttas-field ttas-span-2">
                                                <span>{{ tr('allowedCallers') }}</span>
                                                <input class="text_pole" v-model="draft.delegation.allowedCallersCsv" :disabled="isBuiltinProfile" />
                                                <small class="ttas-field-hint">
                                                    <i class="fa-solid fa-circle-info" aria-hidden="true"></i>
                                                    <span>{{ tr('allowedCallersHint') }}</span>
                                                </small>
                                            </label>
                                        </div>
                                    </div>
                                </div>

                                <div v-if="profileEditMode === 'subagent'" class="ttas-section" data-ttas-profile-section="subagent-access">
                                    <div class="ttas-section-title">
                                        <i class="fa-solid fa-people-arrows"></i>
                                        <h4>{{ tr('subAgentAccess') }}</h4>
                                    </div>
                                    <div class="ttas-delegation-panel">
                                        <label class="ttas-switch-row">
                                            <input type="checkbox" :checked="isCallableAsSubAgent" :disabled="isBuiltinProfile" @change="setCallableAsSubAgent($event.target.checked)" />
                                            <span>
                                                <strong>{{ tr('callableSubAgentToggle') }}</strong>
                                                <small>{{ tr('callableSubAgentHint') }}</small>
                                            </span>
                                        </label>
                                        <div v-if="isCallableAsSubAgent" class="ttas-form-grid ttas-delegation-controls">
                                            <label class="ttas-field ttas-span-2">
                                                <span>{{ tr('agentFacingDescription') }}</span>
                                                <textarea class="text_pole textarea_compact" rows="4" v-model="draft.delegation.descriptionForAgents" :disabled="isBuiltinProfile" :placeholder="draft.description"></textarea>
                                                <small class="ttas-field-hint">
                                                    <i class="fa-solid fa-circle-info" aria-hidden="true"></i>
                                                    <span>{{ tr('agentFacingDescriptionHint') }}</span>
                                                </small>
                                            </label>
                                            <label class="ttas-field ttas-span-2">
                                                <span>{{ tr('allowedCallers') }}</span>
                                                <input class="text_pole" v-model="draft.delegation.allowedCallersCsv" :disabled="isBuiltinProfile" />
                                                <small class="ttas-field-hint">
                                                    <i class="fa-solid fa-circle-info" aria-hidden="true"></i>
                                                    <span>{{ tr('allowedCallersHint') }}</span>
                                                </small>
                                            </label>
                                        </div>
                                    </div>
                                </div>

                                <div class="ttas-section" data-ttas-profile-section="run">
                                    <div class="ttas-section-title">
                                        <i class="fa-solid fa-gauge-high"></i>
                                        <h4>{{ tr('runPolicy') }}</h4>
                                    </div>
                                    <div class="ttas-form-grid">
                                        <label class="ttas-field">
                                            <span>{{ tr('presentation') }}</span>
                                            <select :value="draft.run.presentation" :disabled="isBuiltinProfile || isSubAgentPresentationLocked" @change="setRunPresentation($event.target.value)">
                                                <option value="foreground">{{ tr('foreground') }}</option>
                                                <option value="background">{{ tr('background') }}</option>
                                            </select>
                                        </label>
                                        <label class="ttas-field">
                                            <span>{{ tr('planMode') }}</span>
                                            <select v-model="draft.plan.mode" :disabled="isBuiltinProfile">
                                                <option value="none">{{ tr('none') }}</option>
                                            </select>
                                        </label>
                                        <label class="ttas-field">
                                            <span>{{ tr('maxRounds') }}</span>
                                            <input class="text_pole" type="number" min="1" v-model.number="draft.tools.maxRounds" :disabled="isBuiltinProfile" />
                                        </label>
                                        <label class="ttas-field">
                                            <span>{{ tr('maxToolCalls') }}</span>
                                            <input class="text_pole" type="number" min="1" v-model.number="draft.tools.maxCallsPerRun" :disabled="isBuiltinProfile" />
                                        </label>
                                        <label class="ttas-field">
                                            <span>{{ tr('modelRetries') }}</span>
                                            <input class="text_pole" type="number" min="0" v-model.number="draft.run.modelRetry.maxRetries" :disabled="isBuiltinProfile" />
                                        </label>
                                        <label class="ttas-field">
                                            <span>{{ tr('retryIntervalMs') }}</span>
                                            <input class="text_pole" type="number" min="1" v-model.number="draft.run.modelRetry.intervalMs" :disabled="isBuiltinProfile" />
                                        </label>
                                    </div>
                                </div>

                                <div class="ttas-section" data-ttas-profile-section="context">
                                    <div class="ttas-section-title">
                                        <i class="fa-solid fa-layer-group"></i>
                                        <h4>{{ tr('initialContext') }}</h4>
                                    </div>
                                    <div class="ttas-form-grid">
                                        <label class="ttas-field">
                                            <span>{{ tr('initialChatHistoryMessages') }}</span>
                                            <input class="text_pole" type="number" step="1" v-model.number="draft.context.initialChatHistoryMessages" :disabled="isBuiltinProfile" />
                                            <small class="ttas-field-hint">
                                                <i class="fa-solid fa-circle-info" aria-hidden="true"></i>
                                                <span>{{ tr('initialChatHistoryMessagesHint') }}</span>
                                            </small>
                                        </label>
                                        <label class="checkbox_label ttas-field">
                                            <span>{{ tr('includeActivatedWorldInfo') }}</span>
                                            <input type="checkbox" v-model="draft.context.includeActivatedWorldInfo" :disabled="isBuiltinProfile" />
                                        </label>
                                    </div>
                                </div>

                                <div class="ttas-section" data-ttas-profile-section="prompt">
                                    <div class="ttas-section-title">
                                        <i class="fa-solid fa-terminal"></i>
                                        <h4>{{ tr('prompt') }}</h4>
                                    </div>
                                    <label class="ttas-field">
                                        <span>{{ tr('agentSystemPrompt') }}</span>
                                        <textarea class="text_pole textarea_compact ttas-system-prompt-textarea" rows="12" :value="agentSystemPromptEditorValue" :placeholder="agentSystemPromptPlaceholder" :disabled="isBuiltinProfile" @input="setAgentSystemPromptDraft"></textarea>
                                    </label>
                                </div>

                                <div class="ttas-section" data-ttas-profile-section="tools">
                                    <div class="ttas-section-title">
                                        <i class="fa-solid fa-screwdriver-wrench"></i>
                                        <h4>{{ tr('capabilityMatrix') }}</h4>
                                    </div>
                                    <div class="ttas-tool-workbench">
                                        <div class="ttas-tool-groups">
                                            <div v-for="group in toolGroupsWithTools" :key="group.id" class="ttas-tool-group">
                                                <header>
                                                    <i class="fa-solid" :class="group.icon"></i>
                                                    <strong>{{ tr(group.labelKey) }}</strong>
                                                    <span>{{ enabledToolCount(group.tools) }}/{{ group.tools.length }}</span>
                                                </header>
                                                <div class="ttas-tool-list">
                                                    <div
                                                        v-for="tool in group.tools"
                                                        :key="tool"
                                                        class="ttas-tool-row"
                                                        :class="{ active: selectedToolName === tool, enabled: draft.tools.allow.includes(tool), customized: toolHasDescriptionOverride(tool) }"
                                                    >
                                                        <input
                                                            type="checkbox"
                                                            :checked="draft.tools.allow.includes(tool)"
                                                            :disabled="isBuiltinProfile"
                                                            @change="toggleToolAllowed(tool, $event)"
                                                        />
                                                        <button type="button" class="ttas-tool-select" @click="selectTool(tool)">
                                                            <strong>{{ toolTitle(tool) }}</strong>
                                                            <span>{{ tool }}</span>
                                                        </button>
                                                        <i v-if="toolHasDescriptionOverride(tool)" class="fa-solid fa-pen-nib ttas-tool-custom-marker" :title="tr('customizedTool')"></i>
                                                    </div>
                                                </div>
                                            </div>
                                        </div>

                                        <aside v-if="selectedToolSpec" class="ttas-tool-editor-panel">
                                            <header class="ttas-tool-editor-header">
                                                <div>
                                                    <div class="ttas-eyebrow">{{ selectedToolName }}</div>
                                                    <h5>{{ selectedToolSpec.title }}</h5>
                                                </div>
                                                <button
                                                    type="button"
                                                    class="menu_button menu_button_icon"
                                                    :disabled="isBuiltinProfile || !toolHasDescriptionOverride(selectedToolName)"
                                                    @click="resetToolDescriptionOverride(selectedToolName)"
                                                >
                                                    <i class="fa-solid fa-rotate-left"></i>
                                                    <span>{{ tr('reset') }}</span>
                                                </button>
                                            </header>

                                            <div class="ttas-tool-badge-row">
                                                <span class="ttas-tool-model-name">{{ toolModelName(selectedToolName) }}</span>
                                                <span v-if="toolSource(selectedToolName)">{{ toolSource(selectedToolName) }}</span>
                                                <span v-for="badge in toolBadges(selectedToolName)" :key="badge.key" :class="'ttas-tool-badge-' + badge.key">{{ badge.label }}</span>
                                                <span v-if="!selectedToolEnabled" class="ttas-tool-badge-disabled">{{ tr('disabledTool') }}</span>
                                            </div>

                                            <div class="ttas-tool-default-description">
                                                <span>{{ tr('defaultDescription') }}</span>
                                                <p>{{ selectedToolSpec.description }}</p>
                                            </div>

                                            <label class="ttas-field">
                                                <span>{{ tr('customToolDescription') }}</span>
                                                <textarea
                                                    class="text_pole textarea_compact ttas-tool-description-textarea"
                                                    rows="5"
                                                    :value="getToolDescriptionOverride(selectedToolName)"
                                                    :placeholder="selectedToolSpec.description"
                                                    :disabled="isBuiltinProfile || !selectedToolEnabled"
                                                    @input="setToolDescriptionOverride(selectedToolName, $event.target.value)"
                                                ></textarea>
                                            </label>

                                            <div class="ttas-tool-property-list">
                                                <div class="ttas-tool-property-title">
                                                    <i class="fa-solid fa-sliders"></i>
                                                    <strong>{{ tr('toolParameters') }}</strong>
                                                </div>
                                                <div v-if="selectedToolProperties.length === 0" class="ttas-empty">{{ tr('noToolParameters') }}</div>
                                                <div v-for="property in selectedToolProperties" :key="property.name" class="ttas-tool-property-row">
                                                    <div class="ttas-tool-property-meta">
                                                        <code>{{ property.name }}</code>
                                                        <span>{{ property.type }}</span>
                                                        <em v-if="property.required">{{ tr('required') }}</em>
                                                    </div>
                                                    <p v-if="property.description">{{ property.description }}</p>
                                                    <div class="ttas-tool-property-edit">
                                                        <textarea
                                                            class="text_pole textarea_compact"
                                                            rows="3"
                                                            :value="getToolPropertyDescriptionOverride(selectedToolName, property.name)"
                                                            :placeholder="property.description"
                                                            :disabled="isBuiltinProfile || !selectedToolEnabled"
                                                            @input="setToolPropertyDescriptionOverride(selectedToolName, property.name, $event.target.value)"
                                                        ></textarea>
                                                        <button
                                                            type="button"
                                                            class="menu_button menu_button_icon"
                                                            :disabled="isBuiltinProfile || !getToolPropertyDescriptionOverride(selectedToolName, property.name)"
                                                            @click="resetToolPropertyDescriptionOverride(selectedToolName, property.name)"
                                                        >
                                                            <i class="fa-solid fa-rotate-left"></i>
                                                            <span>{{ tr('reset') }}</span>
                                                        </button>
                                                    </div>
                                                </div>
                                            </div>
                                        </aside>
                                    </div>
                                </div>

                                <div class="ttas-section" data-ttas-profile-section="skills">
                                    <div class="ttas-section-title">
                                        <i class="fa-solid fa-book"></i>
                                        <h4>{{ tr('skillAccess') }}</h4>
                                    </div>
                                    <div class="ttas-form-grid">
                                        <label class="ttas-field">
                                            <span>{{ tr('visibleSkills') }}</span>
                                            <input class="text_pole" v-model="draft.skills.visibleCsv" :disabled="isBuiltinProfile" />
                                        </label>
                                        <label class="ttas-field">
                                            <span>{{ tr('deniedSkills') }}</span>
                                            <input class="text_pole" v-model="draft.skills.denyCsv" :disabled="isBuiltinProfile" />
                                        </label>
                                        <label class="ttas-field">
                                            <span>{{ tr('maxCharsPerCall') }}</span>
                                            <input class="text_pole" type="number" min="1" v-model.number="draft.skills.maxReadCharsPerCall" :disabled="isBuiltinProfile" />
                                        </label>
                                        <label class="ttas-field">
                                            <span>{{ tr('maxCharsPerRun') }}</span>
                                            <input class="text_pole" type="number" min="1" v-model.number="draft.skills.maxReadCharsPerRun" :disabled="isBuiltinProfile" />
                                        </label>
                                    </div>
                                </div>

                                <div class="ttas-section" data-ttas-profile-section="workspace">
                                    <div class="ttas-section-title">
                                        <i class="fa-solid fa-folder-tree"></i>
                                        <h4>{{ tr('workspaceAccess') }}</h4>
                                    </div>
                                    <div class="ttas-root-grid">
                                        <div v-for="root in workspaceRoots" :key="root" class="ttas-root-row">
                                            <div class="ttas-root-name">
                                                <i class="fa-solid" :class="workspaceRootIcon(root)"></i>
                                                <strong>{{ root }}</strong>
                                            </div>
                                            <label class="checkbox_label">
                                                <input type="checkbox" :value="root" v-model="draft.workspace.visibleRoots" @change="syncWritableRoots" :disabled="isBuiltinProfile" />
                                                <span>{{ tr('visible') }}</span>
                                            </label>
                                            <label class="checkbox_label">
                                                <input type="checkbox" :value="root" v-model="draft.workspace.writableRoots" :disabled="isBuiltinProfile || !draft.workspace.visibleRoots.includes(root)" />
                                                <span>{{ tr('writable') }}</span>
                                            </label>
                                        </div>
                                    </div>
                                </div>

                                <div v-if="profileEditMode === 'main'" class="ttas-section" data-ttas-profile-section="output">
                                    <div class="ttas-section-title">
                                        <i class="fa-solid fa-file-lines"></i>
                                        <h4>{{ tr('outputArtifact') }}</h4>
                                    </div>
                                    <div class="ttas-form-grid">
                                        <label class="ttas-field">
                                            <span>{{ tr('messageBodyPath') }}</span>
                                            <input class="text_pole" v-model="draft.output.artifacts[0].path" :disabled="isBuiltinProfile" />
                                        </label>
                                        <label class="ttas-field">
                                            <span>{{ tr('kind') }}</span>
                                            <input class="text_pole" v-model="draft.output.artifacts[0].kind" :disabled="isBuiltinProfile" />
                                        </label>
                                    </div>
                                </div>

                                <div class="ttas-section ttas-json-section" data-ttas-profile-section="json">
                                    <div class="ttas-pane-header">
                                        <div class="ttas-section-title">
                                            <i class="fa-solid fa-code"></i>
                                            <h4>{{ tr('advancedJson') }}</h4>
                                        </div>
                                        <div class="ttas-toolbar">
                                            <button type="button" class="menu_button" @click="refreshDraftJson">{{ tr('refreshJson') }}</button>
                                            <button type="button" class="menu_button" @click="applyDraftJson" :disabled="isBuiltinProfile">{{ tr('applyJson') }}</button>
                                        </div>
                                    </div>
                                    <textarea class="text_pole ttas-json" v-model="draftJson" :readonly="isBuiltinProfile"></textarea>
                                </div>
                            </div>
                        </div>
                    </section>
                    <section v-else-if="activeTab === 'runs'" key="runs" class="ttas-panel">
                        <RunHistoryPanel />
                    </section>
                    </transition>
                </div>
            </div>
        `,
    };
}
