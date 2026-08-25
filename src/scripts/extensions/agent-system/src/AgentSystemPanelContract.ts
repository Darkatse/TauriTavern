import {
    AGENT_DELEGATION_TOOLS,
    DEFAULT_PROFILE_ID,
    KNOWN_TOOLS,
    RUNTIME_ONLY_TOOLS,
} from './constants.js';
import { prettyJson } from './host-api.js';
import type { AgentSystemTr } from './i18n.js';
import { defaultProfile, profileForEdit, type AgentProfileDraft } from './profile-model';
import type { AgentModelTarget } from './model-target-connection.js';
import type { AgentSystemSettings } from './AgentSystemEntryController';

export const PROFILE_EXPORT_CONTENT_TYPE = 'application/json';
export const CHAT_COMPLETION_PRESET_API_ID = 'openai';

export type Tr = AgentSystemTr;

export type AgentToolCatalogDiagnostic = {
    toolId?: string;
    code: string;
    message: string;
};

export type AgentProfileEditMode = 'main' | 'subagent';
export type AgentProfileSectionId =
    | 'identity'
    | 'binding'
    | 'main-delegation'
    | 'subagent-access'
    | 'run'
    | 'context'
    | 'prompt'
    | 'tools'
    | 'skills'
    | 'workspace'
    | 'output'
    | 'json';

export type AgentSystemPanelTabId = 'profiles' | 'runs';

export const PANEL_TABS: ReadonlyArray<{ id: AgentSystemPanelTabId; labelKey: string; icon: string }> = Object.freeze([
    { id: 'profiles', labelKey: 'profiles', icon: 'fa-id-card-clip' },
    { id: 'runs', labelKey: 'runs', icon: 'fa-clock-rotate-left' },
]);

export const PROFILE_EDIT_MODES: ReadonlyArray<{ id: AgentProfileEditMode; labelKey: string; icon: string }> = Object.freeze([
    { id: 'main', labelKey: 'mainAgent', icon: 'fa-compass-drafting' },
    { id: 'subagent', labelKey: 'subAgent', icon: 'fa-people-arrows' },
]);

export type AgentProfileSection = {
    id: AgentProfileSectionId;
    labelKey: string;
    icon: string;
    modes: readonly AgentProfileEditMode[];
};

export const PROFILE_SECTIONS: readonly AgentProfileSection[] = Object.freeze([
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

export const PROFILE_DIAGNOSTIC_CODES = Object.freeze({
    PROFILE_CONTRACT_INVALID: 'agent.profile_contract_invalid',
    PRESET_API_UNSUPPORTED: 'agent.profile_preset_api_unsupported',
    PRESET_MISSING: 'agent.profile_preset_missing',
    MODEL_REQUIRES_CONFIGURATION: 'agent.profile_model_requires_configuration',
    MODEL_CONNECTION_MISSING: 'agent.profile_model_connection_missing',
    MODEL_CONNECTION_INVALID: 'agent.profile_model_connection_invalid',
});

export const PROFILE_TOOL_MATRIX_HIDDEN: ReadonlySet<string> = new Set([
    ...AGENT_DELEGATION_TOOLS,
    ...RUNTIME_ONLY_TOOLS,
]);

export type AgentToolGroup = {
    id: string;
    labelKey: string;
    icon: string;
    tools: readonly string[];
};

export const TOOL_GROUPS: readonly AgentToolGroup[] = Object.freeze([
    {
        id: 'context',
        labelKey: 'contextTools',
        icon: 'fa-comments',
        tools: ['builtin:chat.search', 'builtin:chat.read_messages', 'builtin:worldinfo.read_activated'],
    },
    {
        id: 'skills',
        labelKey: 'skillTools',
        icon: 'fa-book-open',
        tools: ['builtin:skill.list', 'builtin:skill.search', 'builtin:skill.read', 'builtin:skill.run_script'],
    },
    {
        id: 'workspace-read',
        labelKey: 'workspaceReadTools',
        icon: 'fa-folder-tree',
        tools: ['builtin:workspace.list_files', 'builtin:workspace.search_files', 'builtin:workspace.read_file'],
    },
    {
        id: 'workspace-write',
        labelKey: 'workspaceWriteTools',
        icon: 'fa-pen-to-square',
        tools: ['builtin:workspace.write_file', 'builtin:workspace.apply_patch'],
    },
    {
        id: 'control',
        labelKey: 'controlTools',
        icon: 'fa-flag-checkered',
        tools: ['builtin:workspace.commit', 'builtin:workspace.finish'],
    },
    {
        id: 'other',
        labelKey: 'otherTools',
        icon: 'fa-dice',
        tools: ['builtin:dice.roll'],
    },
]);

const WORKSPACE_ROOT_ICONS: Readonly<Record<string, string>> = Object.freeze({
    output: 'fa-message',
    scratch: 'fa-note-sticky',
    plan: 'fa-list-check',
    summaries: 'fa-layer-group',
    persist: 'fa-database',
});

export function workspaceRootIcon(root: string): string {
    return WORKSPACE_ROOT_ICONS[root] || 'fa-folder';
}

export function isProfileEditMode(mode: string): mode is AgentProfileEditMode {
    return PROFILE_EDIT_MODES.some((item) => item.id === mode);
}

export function firstProfileSectionIdForMode(mode: AgentProfileEditMode): AgentProfileSectionId {
    const section = PROFILE_SECTIONS.find((item) => item.modes.includes(mode));
    if (!section) {
        throw new Error(`Unsupported Agent profile edit mode: ${mode}`);
    }
    return section.id;
}

export function preferredProfileEditMode(profile: { run?: { directRunnable?: boolean } } | null): AgentProfileEditMode {
    return profile?.run?.directRunnable === false ? 'subagent' : 'main';
}

export function profileSectionsForMode(mode: AgentProfileEditMode): AgentProfileSection[] {
    return PROFILE_SECTIONS.filter((section) => section.modes.includes(mode));
}

export function isBuiltinProfile(draft: AgentProfileDraft): boolean {
    return draft.id === DEFAULT_PROFILE_ID;
}

export function isSubAgentOnly(draft: AgentProfileDraft): boolean {
    return draft.run.directRunnable === false;
}

export function isCallableAsSubAgent(draft: AgentProfileDraft): boolean {
    return Boolean(draft.delegation.callable && draft.delegation.allowAsSubagent);
}

export function isCallableAsHandoffTarget(draft: AgentProfileDraft): boolean {
    return Boolean(draft.delegation.callable && draft.delegation.allowAsHandoffTarget);
}

export function activeProfileIdOf(settings: AgentSystemSettings): string {
    return settings.activeProfileId || DEFAULT_PROFILE_ID;
}

export function activeProfileOptions(profiles: readonly TauriTavernAgentProfileSummary[]): TauriTavernAgentProfileSummary[] {
    return profiles.filter((profile) => profile.directRunnable !== false);
}

/** `v-model.number` semantics: an emptied input stays '', never becomes 0. */
export function parseNumberInput(raw: string): number | '' {
    if (raw === '') {
        return '';
    }
    return Number(raw);
}

function firstKnownToolId(): string {
    const [first] = KNOWN_TOOLS;
    if (!first) {
        throw new Error('Agent System known tool list is empty');
    }
    return first;
}

export const FIRST_KNOWN_TOOL_ID = firstKnownToolId();

/**
 * Immutable controller snapshot. The draft is the only deeply mutable-looking
 * structure; every edit replaces it via clone+commit so React re-renders.
 */
export type AgentSystemPanelSnapshot = {
    initialized: boolean;
    loading: boolean;
    saving: boolean;
    error: string;
    settings: AgentSystemSettings;
    profiles: TauriTavernAgentProfileSummary[];
    editingProfileId: string;
    profileEditMode: AgentProfileEditMode;
    activeProfileSectionId: AgentProfileSectionId;
    // Increments on each rail click; the App scrolls after commit.
    profileSectionScrollRequest: number;
    draft: AgentProfileDraft;
    draftJson: string;
    externalProfileChangePending: boolean;
    resolvedAgentSystemPrompt: string;
    profilePreviewError: string;
    profileHealth: TauriTavernAgentProfileHealth | null;
    profileDiagnosticError: string;
    profileRuntimeStateJson: string;
    toolItems: TauriTavernAgentToolCatalogItem[];
    toolCatalogDiagnostics: AgentToolCatalogDiagnostic[];
    toolIds: string[];
    selectedToolId: string;
    presetOptions: string[];
    modelTargets: AgentModelTarget[];
};

export type AgentSystemPanelControllerDeps = {
    loadSettings: () => Promise<AgentSystemSettings>;
    patchSettings: (
        current: AgentSystemSettings,
        patch: Partial<AgentSystemSettings>,
    ) => Promise<AgentSystemSettings>;
    // Lazy so a missing Host API surfaces at action time like the Vue panel.
    getProfilesApi: () => TauriTavernAgentProfilesApi;
    listTools: () => Promise<{
        tools: TauriTavernAgentToolCatalogItem[];
        diagnostics: AgentToolCatalogDiagnostic[];
    }>;
    listPresetOptions: () => string[];
    listModelTargets: () => AgentModelTarget[];
    saveModelTargetConnection: (target: AgentModelTarget) => Promise<unknown>;
    subscribeProfilesChanged: (listener: () => void) => () => void;
    subscribeModelTargetsChanged: (listener: () => void) => () => void;
    subscribeLlmConnectionsChanged: (listener: () => void) => () => void;
    confirmAction: (message: string) => Promise<boolean>;
    downloadBlob: (blob: Blob, fileName: string) => Promise<{ mode?: string; completed?: boolean } | undefined>;
    notifyError: (error: unknown) => void;
    notifyWarning: (message: string) => void;
    notifySuccess: (message: string) => void;
    // Fired when the runs tab becomes visible (init or tab switch).
    onRunsTabActivated: () => void;
    tr: Tr;
};

export function createInitialPanelSnapshot(): AgentSystemPanelSnapshot {
    const draft = profileForEdit(defaultProfile());
    return {
        initialized: false,
        loading: false,
        saving: false,
        error: '',
        settings: {
            agentModeEnabled: false,
            chatInputToggleHidden: false,
            activeProfileId: DEFAULT_PROFILE_ID,
            editingProfileId: DEFAULT_PROFILE_ID,
            activeTab: 'profiles',
            runTimelineHeightPx: null,
        },
        profiles: [],
        editingProfileId: DEFAULT_PROFILE_ID,
        // UI-only editor view. The saved profile role is owned by run/delegation policy.
        profileEditMode: 'main',
        activeProfileSectionId: firstProfileSectionIdForMode('main'),
        profileSectionScrollRequest: 0,
        draft,
        draftJson: prettyJson(defaultProfile()),
        externalProfileChangePending: false,
        resolvedAgentSystemPrompt: '',
        profilePreviewError: '',
        profileHealth: null,
        profileDiagnosticError: '',
        profileRuntimeStateJson: '',
        toolItems: [],
        toolCatalogDiagnostics: [],
        toolIds: [...KNOWN_TOOLS],
        selectedToolId: FIRST_KNOWN_TOOL_ID,
        presetOptions: [],
        modelTargets: [],
    };
}
