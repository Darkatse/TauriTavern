import { AGENT_DELEGATION_TOOLS, DEFAULT_PROFILE_ID, KNOWN_TOOLS, RUNTIME_ONLY_TOOLS, WORKSPACE_ROOTS } from './constants.js';
import { clone } from './host-api.js';
import { translateAgentSystem as tr } from './i18n.js';
import {
    DEFAULT_AGENT_CONTEXT_POLICY,
    normalizeAgentContextPolicy,
} from '../../../tauritavern/agent/agent-context-policy.js';

export function normalizeProfileId(value) {
    return String(value || '')
        .trim()
        .toLowerCase()
        .replace(/[^a-z0-9_-]+/g, '-')
        .replace(/^-+|-+$/g, '')
        .slice(0, 128);
}

function parseCsv(value) {
    return String(value || '')
        .split(',')
        .map((item) => item.trim())
        .filter(Boolean);
}

function joinCsv(values) {
    return Array.isArray(values) ? values.join(', ') : '';
}

function isPlainObject(value) {
    return value !== null && typeof value === 'object' && !Array.isArray(value);
}

function normalizeToolDescriptions(value) {
    if (value == null) {
        return {};
    }
    if (!isPlainObject(value)) {
        throw new Error('tools.toolDescriptions must be an object');
    }

    const normalized = {};
    for (const [toolName, override] of Object.entries(value)) {
        if (!isPlainObject(override)) {
            throw new Error(`tools.toolDescriptions.${toolName} must be an object`);
        }

        const description = String(override.description || '').trim();
        const properties = {};
        if (override.properties != null) {
            if (!isPlainObject(override.properties)) {
                throw new Error(`tools.toolDescriptions.${toolName}.properties must be an object`);
            }
            for (const [property, propertyDescription] of Object.entries(override.properties)) {
                const trimmed = String(propertyDescription || '').trim();
                if (trimmed) {
                    properties[property] = trimmed;
                }
            }
        }

        if (description || Object.keys(properties).length > 0) {
            normalized[toolName] = {
                ...(description ? { description } : {}),
                ...(Object.keys(properties).length > 0 ? { properties } : {}),
            };
        }
    }

    return normalized;
}

function normalizePresetBinding(value) {
    const binding = isPlainObject(value) ? { ...value } : {};
    const mode = String(binding.mode || 'currentPromptSnapshot').trim() || 'currentPromptSnapshot';
    if (mode === 'currentPromptSnapshot' || mode === 'none') {
        return {
            mode,
            required: false,
        };
    }

    if (mode !== 'ref') {
        throw new Error(`preset.mode is unsupported: ${mode}`);
    }

    const ref = isPlainObject(binding.ref) ? binding.ref : {};
    return {
        mode: 'ref',
        ref: {
            apiId: String(ref.apiId || '').trim(),
            name: String(ref.name || '').trim(),
        },
        required: Boolean(binding.required),
    };
}

function normalizeModelBinding(value) {
    const binding = isPlainObject(value) ? { ...value } : {};
    const mode = String(binding.mode || 'currentPromptSnapshot').trim() || 'currentPromptSnapshot';
    if (mode === 'currentPromptSnapshot') {
        return {
            mode: 'currentPromptSnapshot',
        };
    }

    if (mode !== 'connectionRef') {
        throw new Error(`model.mode is unsupported: ${mode}`);
    }

    return {
        mode: 'connectionRef',
        connectionRef: String(binding.connectionRef || '').trim(),
        modelId: String(binding.modelId || '').trim(),
    };
}

function normalizeRunPolicy(value) {
    const policy = isPlainObject(value) ? { ...value } : {};
    const presentation = String(policy.presentation || 'foreground').trim() || 'foreground';
    if (presentation !== 'foreground' && presentation !== 'background') {
        throw new Error(`run.presentation is unsupported: ${presentation}`);
    }
    const directRunnable = policy.directRunnable !== false;
    const modelRetry = isPlainObject(policy.modelRetry) ? policy.modelRetry : {};

    return {
        presentation: directRunnable ? presentation : 'background',
        directRunnable,
        modelRetry: {
            maxRetries: Number(modelRetry.maxRetries ?? 3),
            intervalMs: Number(modelRetry.intervalMs ?? 3000),
        },
    };
}

function defaultDelegationPolicy() {
    return {
        canDelegate: false,
        canHandoff: false,
        callable: false,
        allowAsSubagent: false,
        allowAsHandoffTarget: false,
        allowNestedDelegation: false,
        allowedCallers: ['*'],
        descriptionForAgents: null,
        maxConcurrentInvocations: 3,
        maxInvocationsPerRun: 8,
        resultBudgetTokens: 8000,
        maxHandoffDepth: 5,
    };
}

function normalizeDelegationPolicy(value) {
    const defaults = defaultDelegationPolicy();
    const policy = isPlainObject(value) ? { ...value } : {};
    const allowedCallers = Object.prototype.hasOwnProperty.call(policy, 'allowedCallersCsv')
        ? parseCsv(policy.allowedCallersCsv)
        : (Array.isArray(policy.allowedCallers) ? policy.allowedCallers.map((caller) => String(caller || '').trim()).filter(Boolean) : defaults.allowedCallers);
    const description = String(policy.descriptionForAgents || '').trim();

    return {
        canDelegate: Boolean(policy.canDelegate),
        canHandoff: Boolean(policy.canHandoff),
        callable: Boolean(policy.callable),
        allowAsSubagent: Boolean(policy.allowAsSubagent),
        allowAsHandoffTarget: Boolean(policy.allowAsHandoffTarget),
        allowNestedDelegation: Boolean(policy.allowNestedDelegation),
        allowedCallers,
        descriptionForAgents: description || null,
        maxConcurrentInvocations: Number(policy.maxConcurrentInvocations ?? defaults.maxConcurrentInvocations),
        maxInvocationsPerRun: Number(policy.maxInvocationsPerRun ?? defaults.maxInvocationsPerRun),
        resultBudgetTokens: Number(policy.resultBudgetTokens ?? defaults.resultBudgetTokens),
        maxHandoffDepth: Number(policy.maxHandoffDepth ?? defaults.maxHandoffDepth),
    };
}

function applyDelegationToolPolicy(profile) {
    const delegation = profile.delegation || defaultDelegationPolicy();
    const runtimeOnly = new Set(RUNTIME_ONLY_TOOLS);
    const allow = new Set((Array.isArray(profile.tools?.allow) ? profile.tools.allow : [])
        .filter((tool) => !runtimeOnly.has(tool)));

    if (delegation.canDelegate) {
        for (const tool of AGENT_DELEGATION_TOOLS) {
            allow.add(tool);
        }
    } else {
        allow.delete('agent.delegate');
        allow.delete('agent.await');
        if (!delegation.canHandoff) {
            allow.delete('agent.list');
        }
    }

    const preferredOrder = [
        ...AGENT_DELEGATION_TOOLS,
        ...KNOWN_TOOLS,
    ];
    profile.tools.allow = [
        ...preferredOrder.filter((tool) => allow.has(tool)),
        ...[...allow].filter((tool) => !preferredOrder.includes(tool)),
    ];
}

export function defaultProfile(id = DEFAULT_PROFILE_ID) {
    const profileId = normalizeProfileId(id) || DEFAULT_PROFILE_ID;
    const profile = {
        schemaVersion: 2,
        kind: 'tauritavern.agentProfile',
        id: profileId,
        displayName: profileId === DEFAULT_PROFILE_ID ? tr('defaultWriter') : tr('newAgentProfile'),
        description: profileId === DEFAULT_PROFILE_ID ? tr('defaultWriterDescription') : '',
        preset: {
            mode: 'currentPromptSnapshot',
            required: false,
        },
        model: {
            mode: 'currentPromptSnapshot',
        },
        run: {
            presentation: 'foreground',
            directRunnable: true,
            modelRetry: {
                maxRetries: 3,
                intervalMs: 3000,
            },
        },
        context: {
            ...DEFAULT_AGENT_CONTEXT_POLICY,
        },
        delegation: defaultDelegationPolicy(),
        instructions: {
            agentSystemPrompt: null,
        },
        tools: {
            allow: [...KNOWN_TOOLS],
            deny: [],
            toolDescriptions: {},
            maxRounds: 80,
            maxCallsPerRun: 80,
            maxCallsPerTool: {},
        },
        skills: {
            visible: ['*'],
            deny: [],
            maxReadCharsPerCall: 20000,
            maxReadCharsPerRun: 80000,
        },
        workspace: {
            visibleRoots: [...WORKSPACE_ROOTS],
            writableRoots: [...WORKSPACE_ROOTS],
        },
        plan: {
            mode: 'none',
            beta: true,
            nodes: [],
        },
        output: {
            artifacts: [
                {
                    id: 'main',
                    path: 'output/main.md',
                    kind: 'markdown',
                    target: 'messageBody',
                    required: true,
                    assemblyOrder: 0,
                },
            ],
        },
    };
    return profile;
}

export function normalizeProfileForSave(profile) {
    const normalized = clone(profile);
    const visibleCsv = Object.prototype.hasOwnProperty.call(normalized.skills, 'visibleCsv')
        ? normalized.skills.visibleCsv
        : joinCsv(normalized.skills.visible);
    const denyCsv = Object.prototype.hasOwnProperty.call(normalized.skills, 'denyCsv')
        ? normalized.skills.denyCsv
        : joinCsv(normalized.skills.deny);

    normalized.id = normalizeProfileId(normalized.id);
    normalized.displayName = String(normalized.displayName || '').trim();
    normalized.description = String(normalized.description || '').trim();
    normalized.schemaVersion = 2;
    normalized.preset = normalizePresetBinding(normalized.preset);
    normalized.model = normalizeModelBinding(normalized.model);
    normalized.run = normalizeRunPolicy(normalized.run);
    normalized.context = normalizeAgentContextPolicy(normalized.context);
    normalized.delegation = normalizeDelegationPolicy(normalized.delegation);
    normalized.tools.maxRounds = Number(normalized.tools.maxRounds);
    normalized.tools.maxCallsPerRun = Number(normalized.tools.maxCallsPerRun);
    normalized.tools.toolDescriptions = normalizeToolDescriptions(normalized.tools.toolDescriptions);
    normalized.skills.maxReadCharsPerCall = Number(normalized.skills.maxReadCharsPerCall);
    normalized.skills.maxReadCharsPerRun = Number(normalized.skills.maxReadCharsPerRun);
    normalized.instructions.agentSystemPrompt = String(normalized.instructions.agentSystemPrompt || '').trim() || null;
    normalized.skills.visible = parseCsv(visibleCsv);
    normalized.skills.deny = parseCsv(denyCsv);
    delete normalized.skills.visibleCsv;
    delete normalized.skills.denyCsv;
    applyDelegationToolPolicy(normalized);
    normalized.output.artifacts = [
        {
            ...normalized.output.artifacts[0],
            id: 'main',
            target: 'messageBody',
            required: true,
            assemblyOrder: 0,
        },
    ];
    return normalized;
}

export function profileForEdit(profile) {
    const draft = clone(profile);
    draft.schemaVersion = Number(draft.schemaVersion || 1) < 2 ? 2 : Number(draft.schemaVersion);
    draft.preset = normalizePresetBinding(draft.preset);
    draft.model = normalizeModelBinding(draft.model);
    draft.run = normalizeRunPolicy(draft.run);
    draft.context = normalizeAgentContextPolicy(draft.context);
    draft.delegation = normalizeDelegationPolicy(draft.delegation);
    draft.delegation.allowedCallersCsv = joinCsv(draft.delegation.allowedCallers);
    draft.tools.toolDescriptions = normalizeToolDescriptions(draft.tools.toolDescriptions);
    draft.skills.visibleCsv = joinCsv(draft.skills.visible);
    draft.skills.denyCsv = joinCsv(draft.skills.deny);
    return draft;
}
