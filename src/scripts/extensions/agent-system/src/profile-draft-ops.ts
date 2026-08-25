import { DEFAULT_PROFILE_ID, RUNTIME_ONLY_TOOLS } from './constants.js';
import { translateAgentSystem as tr } from './i18n.js';
import {
    modelBindingFromTarget,
    type AgentModelTarget,
} from './model-target-connection.js';
import { AGENT_MODEL_REQUIRES_CONFIGURATION } from '../../../tauritavern/agent/agent-profile-portable.js';
import {
    PROFILE_TOOL_MATRIX_HIDDEN,
    CHAT_COMPLETION_PRESET_API_ID,
} from './AgentSystemPanelContract';
import { normalizeDelegationToolAllowList, normalizeProfileId, type AgentProfileDraft } from './profile-model';

export type AgentPresentationMemory = Record<string, TauriTavernAgentRunPresentation>;

export function profilePresentationMemoryKey(draftId: string, editingProfileId: string): string {
    return (draftId || editingProfileId || DEFAULT_PROFILE_ID).trim() || DEFAULT_PROFILE_ID;
}

export function rememberMainAgentPresentation(
    memory: AgentPresentationMemory,
    memoryKey: string,
    presentation: TauriTavernAgentRunPresentation,
): void {
    memory[memoryKey] = presentation || 'foreground';
}

export function applyPresetMode(draft: AgentProfileDraft, mode: string, presetOptions: readonly string[]): void {
    if (mode === 'currentPromptSnapshot') {
        draft.preset = {
            mode: 'currentPromptSnapshot',
            required: false,
        };
        return;
    }
    if (mode === 'none') {
        draft.preset = {
            mode: 'none',
            required: false,
        };
        return;
    }
    if (mode !== 'ref') {
        throw new Error(`Unsupported preset mode: ${mode}`);
    }

    const name = (draft.preset.ref?.name || presetOptions[0] || '').trim();
    draft.preset = {
        mode: 'ref',
        ref: {
            apiId: CHAT_COMPLETION_PRESET_API_ID,
            name,
        },
        required: true,
    };
}

export function applyPresetName(draft: AgentProfileDraft, name: string): void {
    draft.preset = {
        mode: 'ref',
        ref: {
            apiId: CHAT_COMPLETION_PRESET_API_ID,
            name: name.trim(),
        },
        required: true,
    };
}

export function applyModelMode(draft: AgentProfileDraft, mode: string, modelTargets: readonly AgentModelTarget[]): void {
    if (mode === 'currentPromptSnapshot') {
        draft.model = {
            mode: 'currentPromptSnapshot',
        };
        return;
    }
    if (mode === AGENT_MODEL_REQUIRES_CONFIGURATION) {
        draft.model = {
            mode: AGENT_MODEL_REQUIRES_CONFIGURATION,
        };
        return;
    }
    if (mode !== 'connectionRef') {
        throw new Error(`Unsupported model mode: ${mode}`);
    }
    if (draft.model.mode === 'connectionRef') {
        return;
    }
    const [target] = modelTargets;
    if (!target) {
        throw new Error(tr('noSavedModelTargets'));
    }
    draft.model = modelBindingFromTarget(target);
}

export function applyModelTarget(
    draft: AgentProfileDraft,
    modelTargets: readonly AgentModelTarget[],
    targetId: string,
): void {
    const target = modelTargets.find((item) => item.id === targetId);
    if (!target) {
        throw new Error(tr('savedModelTargetNotFound', { id: targetId }));
    }
    draft.model = modelBindingFromTarget(target);
}

export function syncDelegationTools(draft: AgentProfileDraft, catalogToolIds: readonly string[]): void {
    draft.tools.allow = normalizeDelegationToolAllowList(
        draft.tools?.allow,
        draft.delegation,
        catalogToolIds,
    );
}

export function applyCanDelegate(draft: AgentProfileDraft, enabled: boolean, catalogToolIds: readonly string[]): void {
    draft.delegation.canDelegate = Boolean(enabled);
    syncDelegationTools(draft, catalogToolIds);
}

export function applyCanHandoff(draft: AgentProfileDraft, enabled: boolean, catalogToolIds: readonly string[]): void {
    draft.delegation.canHandoff = Boolean(enabled);
    syncDelegationTools(draft, catalogToolIds);
}

export function applyRunPresentation(
    draft: AgentProfileDraft,
    presentation: string,
    memory: AgentPresentationMemory,
    memoryKey: string,
): void {
    if (draft.run.directRunnable === false) {
        throw new Error('SubAgent-only profiles are locked to background presentation.');
    }
    if (presentation !== 'foreground' && presentation !== 'background') {
        throw new Error(`Unsupported Agent run presentation: ${presentation}`);
    }
    draft.run.presentation = presentation;
    rememberMainAgentPresentation(memory, memoryKey, presentation);
}

function applySubAgentOnlyRunPolicy(
    draft: AgentProfileDraft,
    memory: AgentPresentationMemory,
    memoryKey: string,
): void {
    if (!(draft.delegation.callable && draft.delegation.allowAsSubagent)) {
        throw new Error('SubAgent-only run policy requires callable SubAgent delegation.');
    }
    if (!Object.prototype.hasOwnProperty.call(memory, memoryKey)) {
        rememberMainAgentPresentation(memory, memoryKey, draft.run.presentation || 'foreground');
    }
    // Callable SubAgent profiles enter through TaskReturnRequired child invocations, not direct foreground chat runs.
    draft.run.directRunnable = false;
    draft.run.presentation = 'background';
}

function restoreMainAgentPresentation(
    draft: AgentProfileDraft,
    memory: AgentPresentationMemory,
    memoryKey: string,
): void {
    draft.run.presentation = memory[memoryKey] || 'foreground';
}

export function applyCallableAsSubAgent(
    draft: AgentProfileDraft,
    enabled: boolean,
    memory: AgentPresentationMemory,
    memoryKey: string,
): void {
    const isEnabled = Boolean(enabled);
    draft.delegation.allowAsSubagent = isEnabled;
    draft.delegation.callable = isEnabled || Boolean(draft.delegation.allowAsHandoffTarget);
    if (isEnabled) {
        applySubAgentOnlyRunPolicy(draft, memory, memoryKey);
        return;
    }
    if (draft.run.directRunnable === false) {
        draft.run.directRunnable = true;
        restoreMainAgentPresentation(draft, memory, memoryKey);
    }
}

export function applyCallableAsHandoffTarget(
    draft: AgentProfileDraft,
    enabled: boolean,
    memory: AgentPresentationMemory,
    memoryKey: string,
): boolean {
    const isEnabled = Boolean(enabled);
    draft.delegation.allowAsHandoffTarget = isEnabled;
    draft.delegation.callable = isEnabled || Boolean(draft.delegation.allowAsSubagent);
    if (!isEnabled && !draft.delegation.allowAsSubagent && draft.run.directRunnable === false) {
        draft.run.directRunnable = true;
        restoreMainAgentPresentation(draft, memory, memoryKey);
        return true;
    }
    return false;
}

/**
 * Rebuilds tools.allow after a matrix toggle: hidden delegation tools keep
 * their delegation-driven entries, allow-listed tools missing from the catalog
 * survive, and visible tools follow catalog order.
 */
export function applyToolAllowed(
    draft: AgentProfileDraft,
    toolId: string,
    enabled: boolean,
    catalogToolIds: ReadonlySet<string>,
    toolIds: readonly string[],
): void {
    const allow = new Set(draft.tools.allow);
    if (enabled) {
        allow.add(toolId);
    } else {
        allow.delete(toolId);
    }
    const hiddenAllowed = draft.tools.allow
        .filter((tool) => PROFILE_TOOL_MATRIX_HIDDEN.has(tool) && !RUNTIME_ONLY_TOOLS.includes(tool));
    const unavailableAllowed = draft.tools.allow
        .filter((tool) => allow.has(tool) && !catalogToolIds.has(tool) && !PROFILE_TOOL_MATRIX_HIDDEN.has(tool));
    draft.tools.allow = [
        ...hiddenAllowed,
        ...unavailableAllowed,
        ...toolIds.filter((tool) => allow.has(tool)),
    ];
}

function updateToolDescriptionOverride(
    draft: AgentProfileDraft,
    toolId: string,
    mutate: (override: TauriTavernToolDescriptionOverride) => void,
): void {
    const toolDescriptions = { ...(draft.tools.toolDescriptions || {}) };
    const override: TauriTavernToolDescriptionOverride = { ...(toolDescriptions[toolId] || {}) };
    mutate(override);
    if (!override.description && !override.properties) {
        delete toolDescriptions[toolId];
    } else {
        toolDescriptions[toolId] = override;
    }
    draft.tools.toolDescriptions = toolDescriptions;
}

export function applyToolDescriptionOverride(draft: AgentProfileDraft, toolId: string, value: string): void {
    updateToolDescriptionOverride(draft, toolId, (override) => {
        if (value.trim()) {
            override.description = value;
        } else {
            delete override.description;
        }
    });
}

export function applyToolPropertyDescriptionOverride(
    draft: AgentProfileDraft,
    toolId: string,
    property: string,
    value: string,
): void {
    updateToolDescriptionOverride(draft, toolId, (override) => {
        const properties = { ...(override.properties || {}) };
        if (value.trim()) {
            properties[property] = value;
        } else {
            delete properties[property];
        }
        if (Object.keys(properties).length > 0) {
            override.properties = properties;
        } else {
            delete override.properties;
        }
    });
}

export function applyResetToolDescriptionOverride(draft: AgentProfileDraft, toolId: string): void {
    const toolDescriptions = { ...(draft.tools.toolDescriptions || {}) };
    delete toolDescriptions[toolId];
    draft.tools.toolDescriptions = toolDescriptions;
}

export function applyResetToolPropertyDescriptionOverride(
    draft: AgentProfileDraft,
    toolId: string,
    property: string,
): void {
    updateToolDescriptionOverride(draft, toolId, (override) => {
        const properties = { ...(override.properties || {}) };
        delete properties[property];
        if (Object.keys(properties).length > 0) {
            override.properties = properties;
        } else {
            delete override.properties;
        }
    });
}

export function applyWorkspaceRootVisible(draft: AgentProfileDraft, root: string, visible: boolean): void {
    const current = draft.workspace.visibleRoots;
    draft.workspace.visibleRoots = visible
        ? [...current, root]
        : current.filter((item) => item !== root);
    const visibleRoots = new Set(draft.workspace.visibleRoots);
    draft.workspace.writableRoots = draft.workspace.writableRoots.filter((item) => visibleRoots.has(item));
}

export function applyWorkspaceRootWritable(draft: AgentProfileDraft, root: string, writable: boolean): void {
    const current = draft.workspace.writableRoots;
    draft.workspace.writableRoots = writable
        ? [...current, root]
        : current.filter((item) => item !== root);
}

export function nextProfileId(profiles: readonly { id: string }[], base: string): string {
    const normalized = normalizeProfileId(base) || 'agent-profile';
    const ids = new Set(profiles.map((profile) => profile.id));
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
}
