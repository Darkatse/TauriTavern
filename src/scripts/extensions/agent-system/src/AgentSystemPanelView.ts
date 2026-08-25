import { prettyJson } from './host-api.js';
import { findModelTargetForBinding, type AgentModelTarget } from './model-target-connection.js';
import { AGENT_MODEL_REQUIRES_CONFIGURATION } from '../../../tauritavern/agent/agent-profile-portable.js';
import { normalizeProfileForSave, type AgentProfileDraft } from './profile-model';
import {
    PROFILE_DIAGNOSTIC_CODES,
    PROFILE_EDIT_MODES,
    TOOL_GROUPS,
    isBuiltinProfile,
    type AgentProfileEditMode,
    type AgentToolCatalogDiagnostic,
    type AgentToolGroup,
    type Tr,
} from './AgentSystemPanelContract';

export function profileEditModeLabel(mode: AgentProfileEditMode, tr: Tr): string {
    const editMode = PROFILE_EDIT_MODES.find((item) => item.id === mode);
    return editMode ? tr(editMode.labelKey) : tr('mainAgent');
}

export function isProfileRuntimeStateCurrent(draft: AgentProfileDraft, profileRuntimeStateJson: string): boolean {
    return Boolean(profileRuntimeStateJson)
        && prettyJson(normalizeProfileForSave(draft)) === profileRuntimeStateJson;
}

export function agentSystemPromptEditorValue(draft: AgentProfileDraft, resolvedAgentSystemPrompt: string): string {
    if (isBuiltinProfile(draft)) {
        return resolvedAgentSystemPrompt;
    }
    return draft.instructions.agentSystemPrompt ?? '';
}

export function agentSystemPromptPlaceholder(
    draft: AgentProfileDraft,
    resolvedAgentSystemPrompt: string,
    isRuntimeStateCurrent: boolean,
): string {
    if (isBuiltinProfile(draft) || (draft.instructions.agentSystemPrompt || '').trim()) {
        return '';
    }
    return isRuntimeStateCurrent ? resolvedAgentSystemPrompt : '';
}

export function presetSummaryLabel(draft: AgentProfileDraft, tr: Tr): string {
    const preset = draft.preset;
    if (preset.mode === 'ref') {
        return preset.ref?.name || tr('savedPreset');
    }
    if (preset.mode === 'none') {
        return tr('none');
    }
    return tr('currentPromptPreset');
}

export function selectedModelTarget(
    modelTargets: readonly AgentModelTarget[],
    draft: AgentProfileDraft,
): AgentModelTarget | null {
    return findModelTargetForBinding(modelTargets, draft.model);
}

export function modelSummaryLabel(
    draft: AgentProfileDraft,
    modelTargets: readonly AgentModelTarget[],
    tr: Tr,
): string {
    const model = draft.model;
    if (model.mode === AGENT_MODEL_REQUIRES_CONFIGURATION) {
        return tr('modelRequiresConfiguration');
    }
    if (model.mode !== 'connectionRef') {
        return tr('currentChatModel');
    }
    const target = selectedModelTarget(modelTargets, draft);
    if (target) {
        return target.name || target.model || '';
    }
    return model.modelId || model.connectionRef || tr('savedModel');
}

export function hasExternalModelBinding(draft: AgentProfileDraft, modelTargets: readonly AgentModelTarget[]): boolean {
    return draft.model.mode === 'connectionRef' && !selectedModelTarget(modelTargets, draft);
}

export function missingPresetName(draft: AgentProfileDraft, presetOptions: readonly string[]): string {
    const preset = draft.preset;
    if (preset.mode !== 'ref') {
        return '';
    }
    const name = (preset.ref?.name || '').trim();
    if (!name || presetOptions.includes(name)) {
        return '';
    }
    return name;
}

export function delegationSummaryLabel(draft: AgentProfileDraft, profileEditMode: AgentProfileEditMode, tr: Tr): string {
    const delegation = draft.delegation;
    if (profileEditMode === 'subagent') {
        return delegation.callable && delegation.allowAsSubagent ? tr('callableSubAgent') : tr('notCallable');
    }
    const summary: string[] = [];
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
}

export type AgentProfileStat = {
    icon: string;
    label: string;
    value: string;
};

export function profileStatsView(input: {
    draft: AgentProfileDraft;
    toolIds: readonly string[];
    profileEditMode: AgentProfileEditMode;
    modelTargets: readonly AgentModelTarget[];
}, tr: Tr): AgentProfileStat[] {
    const { draft, toolIds, profileEditMode, modelTargets } = input;
    const allowedTools = new Set(Array.isArray(draft.tools?.allow) ? draft.tools.allow : []);
    const enabledToolCount = toolIds.filter((tool) => allowedTools.has(tool)).length;
    const visibleRootCount = Array.isArray(draft.workspace?.visibleRoots) ? draft.workspace.visibleRoots.length : 0;
    const writableRootCount = Array.isArray(draft.workspace?.writableRoots) ? draft.workspace.writableRoots.length : 0;
    return [
        {
            icon: 'fa-scroll',
            label: tr('preset'),
            value: presetSummaryLabel(draft, tr),
        },
        {
            icon: 'fa-microchip',
            label: tr('model'),
            value: modelSummaryLabel(draft, modelTargets, tr),
        },
        {
            icon: profileEditMode === 'subagent' ? 'fa-people-arrows' : 'fa-compass-drafting',
            label: tr('profileView'),
            value: profileEditModeLabel(profileEditMode, tr),
        },
        {
            icon: 'fa-layer-group',
            label: tr('presentation'),
            value: tr(draft.run.presentation || 'foreground'),
        },
        {
            icon: 'fa-diagram-project',
            label: tr('agentCooperation'),
            value: delegationSummaryLabel(draft, profileEditMode, tr),
        },
        {
            icon: 'fa-screwdriver-wrench',
            label: tr('tools'),
            value: `${enabledToolCount}/${toolIds.length}`,
        },
        {
            icon: 'fa-folder-tree',
            label: tr('workspaceRoots'),
            value: `${writableRootCount}/${visibleRootCount}`,
        },
    ];
}

export function availablePresetOptions(presetOptions: readonly string[], draft: AgentProfileDraft): string[] {
    const names = [...presetOptions];
    const selected = draft.preset.mode === 'ref' ? (draft.preset.ref?.name || '').trim() : '';
    if (selected && !names.includes(selected)) {
        names.push(selected);
    }
    return names;
}

export function profileDiagnostics(
    profileHealth: TauriTavernAgentProfileHealth | null,
    isRuntimeStateCurrent: boolean,
): TauriTavernAgentProfileDiagnostic[] {
    if (!isRuntimeStateCurrent || !profileHealth) {
        return [];
    }
    return Array.isArray(profileHealth.diagnostics) ? profileHealth.diagnostics : [];
}

export function profileDiagnosticMessage(diagnostic: TauriTavernAgentProfileDiagnostic, tr: Tr): string {
    const resource: Partial<NonNullable<TauriTavernAgentProfileDiagnostic['resource']>> = diagnostic.resource || {};
    switch (diagnostic.code) {
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
            return diagnostic.message || diagnostic.code || tr('unknownError');
    }
}

export function uniqueMessages(messages: readonly string[]): string[] {
    return [...new Set(messages.filter(Boolean))];
}

export function profileConfigurationWarnings(input: {
    draft: AgentProfileDraft;
    toolCatalogDiagnostics: readonly AgentToolCatalogDiagnostic[];
    profileHealth: TauriTavernAgentProfileHealth | null;
    isRuntimeStateCurrent: boolean;
    profileDiagnosticError: string;
    profilePreviewError: string;
    presetOptions: readonly string[];
    modelTargets: readonly AgentModelTarget[];
}, tr: Tr): string[] {
    const warnings = input.toolCatalogDiagnostics
        .map((diagnostic) => diagnostic.message || diagnostic.code || tr('unknownError'));
    const diagnostics = profileDiagnostics(input.profileHealth, input.isRuntimeStateCurrent);
    if (diagnostics.length > 0) {
        warnings.push(...diagnostics.map((diagnostic) => profileDiagnosticMessage(diagnostic, tr)));
    } else {
        const missingPreset = missingPresetName(input.draft, input.presetOptions);
        if (missingPreset) {
            warnings.push(tr('agentProfilePresetMissing', { name: missingPreset }));
        }
        if (hasExternalModelBinding(input.draft, input.modelTargets)) {
            warnings.push(tr('agentProfileModelBindingMissing', { id: input.draft.model.connectionRef }));
        }
    }
    if (input.isRuntimeStateCurrent && input.profileDiagnosticError) {
        warnings.push(tr('agentProfileDiagnosticsUnavailable', { error: input.profileDiagnosticError }));
    }
    const diagnosticCoversPreview = diagnostics.some((diagnostic) => (
        Array.isArray(diagnostic.blocks) && diagnostic.blocks.includes('preview')
    ));
    if (input.isRuntimeStateCurrent && input.profilePreviewError && !diagnosticCoversPreview) {
        warnings.push(tr('agentProfilePreviewUnavailable', { error: input.profilePreviewError }));
    }
    return uniqueMessages(warnings);
}

export function toolGroupsWithTools(toolIds: readonly string[]): AgentToolGroup[] {
    const groupedTools = new Set<string>();
    const groups = TOOL_GROUPS
        .map((group) => {
            const tools = group.tools.filter((tool) => toolIds.includes(tool));
            tools.forEach((tool) => groupedTools.add(tool));
            return { ...group, tools };
        })
        .filter((group) => group.tools.length > 0);
    const extraTools = toolIds.filter((tool) => !groupedTools.has(tool));
    if (extraTools.length > 0) {
        groups.push({
            id: 'extra',
            labelKey: 'otherTools',
            icon: 'fa-ellipsis',
            tools: extraTools,
        });
    }
    return groups;
}

export function toolItemsById(
    toolItems: readonly TauriTavernAgentToolCatalogItem[],
): Record<string, TauriTavernAgentToolCatalogItem> {
    return Object.fromEntries(toolItems.map((spec) => [spec.id, spec]));
}

export function toolTitle(item: TauriTavernAgentToolCatalogItem | null | undefined, toolId: string): string {
    return item?.title || toolId;
}

export function toolReferenceLabel(item: TauriTavernAgentToolCatalogItem | null | undefined, toolId: string): string {
    return item?.source === 'mcp'
        ? `${String(item.serverDisplayName)} · ${item.nativeName}`
        : toolId;
}

export function toolSource(item: TauriTavernAgentToolCatalogItem | null | undefined, tr: Tr): string {
    return item?.serverDisplayName || item?.source || tr('unavailableTool');
}

function isPlainObject(value: unknown): value is Record<string, unknown> {
    return value !== null && typeof value === 'object' && !Array.isArray(value);
}

export function schemaType(schema: unknown, tr: Tr): string {
    const type = isPlainObject(schema) ? schema.type : undefined;
    if (Array.isArray(type)) {
        return type.join(' | ');
    }
    return (typeof type === 'string' && type) || tr('value');
}

export type AgentToolProperty = {
    name: string;
    schema: unknown;
    required: boolean;
    type: string;
    description: string;
};

export function selectedToolProperties(
    item: TauriTavernAgentToolCatalogItem | null,
    tr: Tr,
): AgentToolProperty[] {
    const properties = item?.inputSchema?.properties ?? {};
    const required = new Set(Array.isArray(item?.inputSchema?.required) ? item.inputSchema.required : []);
    return Object.entries(properties).map(([name, schema]) => ({
        name,
        schema,
        required: required.has(name),
        type: schemaType(schema, tr),
        description: isPlainObject(schema) && typeof schema.description === 'string' ? schema.description : '',
    }));
}

export function toolHasDescriptionOverride(draft: AgentProfileDraft, toolId: string): boolean {
    const override = draft.tools.toolDescriptions?.[toolId];
    return Boolean(override?.description || Object.keys(override?.properties || {}).length > 0);
}

export function getToolDescriptionOverride(draft: AgentProfileDraft, toolId: string): string {
    return draft.tools.toolDescriptions?.[toolId]?.description || '';
}

export function getToolPropertyDescriptionOverride(draft: AgentProfileDraft, toolId: string, property: string): string {
    return draft.tools.toolDescriptions?.[toolId]?.properties?.[property] || '';
}

export type AgentToolBadge = { key: string; label: string };

export function toolBadges(
    draft: AgentProfileDraft,
    item: TauriTavernAgentToolCatalogItem | null,
    toolId: string,
    tr: Tr,
): AgentToolBadge[] {
    const annotations = item?.annotations || {};
    const badges: AgentToolBadge[] = [];
    if (annotations.readOnly) {
        badges.push({ key: 'read', label: tr('readOnlyTool') });
    }
    if (annotations.mutating) {
        badges.push({ key: 'write', label: tr('mutatingTool') });
    }
    if (annotations.control) {
        badges.push({ key: 'control', label: tr('controlTool') });
    }
    if (item?.permission === 'ask') {
        badges.push({ key: 'ask', label: tr('askAutoTool') });
    }
    if (toolHasDescriptionOverride(draft, toolId)) {
        badges.push({ key: 'custom', label: tr('customizedTool') });
    }
    return badges;
}

export function enabledToolCount(draft: AgentProfileDraft, tools: readonly string[]): number {
    const allow = new Set(Array.isArray(draft.tools.allow) ? draft.tools.allow : []);
    return tools.filter((tool) => allow.has(tool)).length;
}

export function modelTargetBadges(target: AgentModelTarget): string[] {
    const badges = [
        String(target.api || '').trim(),
        String(target['custom-api-format'] || '').trim(),
        String(target.model || '').trim(),
    ].filter(Boolean);
    if (target['api-url']) {
        badges.push(String(target['api-url']).trim());
    }
    return badges;
}
