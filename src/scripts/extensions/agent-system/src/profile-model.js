import { DEFAULT_PROFILE_ID, KNOWN_TOOLS, WORKSPACE_ROOTS } from './constants.js';
import { clone } from './host-api.js';
import { translateAgentSystem as tr } from './i18n.js';

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

export function defaultProfile(id = DEFAULT_PROFILE_ID) {
    const profileId = normalizeProfileId(id) || DEFAULT_PROFILE_ID;
    return {
        schemaVersion: 1,
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
            modelRetry: {
                maxRetries: 3,
                intervalMs: 3000,
            },
        },
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
    normalized.preset.required = Boolean(normalized.preset.required);
    normalized.run.modelRetry.maxRetries = Number(normalized.run.modelRetry.maxRetries);
    normalized.run.modelRetry.intervalMs = Number(normalized.run.modelRetry.intervalMs);
    normalized.tools.maxRounds = Number(normalized.tools.maxRounds);
    normalized.tools.maxCallsPerRun = Number(normalized.tools.maxCallsPerRun);
    normalized.skills.maxReadCharsPerCall = Number(normalized.skills.maxReadCharsPerCall);
    normalized.skills.maxReadCharsPerRun = Number(normalized.skills.maxReadCharsPerRun);
    normalized.instructions.agentSystemPrompt = String(normalized.instructions.agentSystemPrompt || '').trim() || null;
    normalized.skills.visible = parseCsv(visibleCsv);
    normalized.skills.deny = parseCsv(denyCsv);
    delete normalized.skills.visibleCsv;
    delete normalized.skills.denyCsv;
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
    draft.skills.visibleCsv = joinCsv(draft.skills.visible);
    draft.skills.denyCsv = joinCsv(draft.skills.deny);
    return draft;
}
