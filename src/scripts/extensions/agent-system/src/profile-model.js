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

function modelToolName(name) {
    return String(name).replace(/\./g, '_');
}

function getMessageBodyPath(profile) {
    const artifacts = Array.isArray(profile?.output?.artifacts) ? profile.output.artifacts : [];
    const artifact = artifacts.find((item) => item?.target === 'messageBody') || artifacts[0];
    return String(artifact?.path || 'output/main.md');
}

function visibleToolNames(profile) {
    const allow = Array.isArray(profile?.tools?.allow) ? profile.tools.allow : KNOWN_TOOLS;
    const deny = new Set(Array.isArray(profile?.tools?.deny) ? profile.tools.deny : []);
    return allow.filter((name) => KNOWN_TOOLS.includes(name) && !deny.has(name));
}

function hasTool(tools, name) {
    return tools.includes(name);
}

function visibleRoots(profile) {
    return Array.isArray(profile?.workspace?.visibleRoots) && profile.workspace.visibleRoots.length
        ? profile.workspace.visibleRoots
        : WORKSPACE_ROOTS;
}

function writableRoots(profile) {
    return Array.isArray(profile?.workspace?.writableRoots) && profile.workspace.writableRoots.length
        ? profile.workspace.writableRoots
        : WORKSPACE_ROOTS;
}

export function buildDefaultAgentSystemPrompt(profile = {}) {
    const tools = visibleToolNames(profile);
    const lines = [
        'TauriTavern Agent Mode is active.',
        'Work through the available Agent tools. Tool results are private run state, not chat messages.',
        `Available tool function names: ${tools.map(modelToolName).join(', ')}.`,
    ];

    if (hasTool(tools, 'chat.search')) {
        lines.push(`Use ${modelToolName('chat.search')} to find relevant prior messages when you need more context. Only query is required.`);
    }
    if (hasTool(tools, 'chat.read_messages')) {
        const sourceHint = hasTool(tools, 'chat.search')
            ? `message indexes from ${modelToolName('chat.search')}`
            : 'exact indexes you already know';
        lines.push(`Use ${modelToolName('chat.read_messages')} with ${sourceHint}. For long messages, read smaller ranges with start_char and max_chars.`);
    }
    if (hasTool(tools, 'worldinfo.read_activated')) {
        lines.push(`Use ${modelToolName('worldinfo.read_activated')} when active lore for this run matters.`);
    }
    if (hasTool(tools, 'skill.list')) {
        lines.push(`Use ${modelToolName('skill.list')} to discover visible Agent Skills when reusable writing, editing, planning, style, or character guidance may help.`);
    }
    if (hasTool(tools, 'skill.search')) {
        lines.push(`Use ${modelToolName('skill.search')} to locate relevant text inside large visible Skill files before reading exact ranges.`);
    }
    if (hasTool(tools, 'skill.read')) {
        lines.push(`Use ${modelToolName('skill.read')} to read SKILL.md first, then read referenced Skill files or ranges only when needed.`);
    }
    if (hasTool(tools, 'workspace.list_files')) {
        lines.push(`Use ${modelToolName('workspace.list_files')} to inspect visible workspace files.`);
    }
    if (hasTool(tools, 'workspace.search_files')) {
        lines.push(`Use ${modelToolName('workspace.search_files')} to find relevant text in visible workspace files such as persist/ memory before reading exact ranges.`);
    }
    if (hasTool(tools, 'workspace.read_file')) {
        lines.push(`Use ${modelToolName('workspace.read_file')} before modifying an existing file. Read output has line numbers; never include line number prefixes in old_string or new_string.`);
    }
    if (hasTool(tools, 'workspace.apply_patch')) {
        lines.push(`Use ${modelToolName('workspace.apply_patch')} for precise edits to existing files. The old_string must match exactly and uniquely unless replace_all is true.`);
    }
    if (hasTool(tools, 'workspace.write_file')) {
        lines.push(`Use ${modelToolName('workspace.write_file')} for new files or complete rewrites.`);
    }
    if (hasTool(tools, 'workspace.commit')) {
        lines.push(`Use ${modelToolName('workspace.commit')} to publish a visible workspace file to the current chat message. With no arguments it replaces the run's chat message with ${getMessageBodyPath(profile)}; mode append appends to the same message and creates it if this run has not committed yet.`);
    }

    const visible = visibleRoots(profile);
    const writable = writableRoots(profile);
    if (visible.includes('persist') && writable.includes('persist')) {
        lines.push('Use persist/ for concise information that should carry into later runs of this same chat, such as durable plot facts, unresolved threads, relationship state, and user style preferences.');
        lines.push('Do not copy full chat history, final replies, tool results, or temporary reasoning into persist/.');
    }

    lines.push(`Visible workspace roots: ${visible.join(', ')}.`);
    lines.push(`Writable workspace roots: ${writable.join(', ')}.`);

    if (profile?.run?.presentation === 'background') {
        lines.push(`Background runs may call ${modelToolName('workspace.finish')} without committing a chat message.`);
    } else {
        lines.push(`Before calling ${modelToolName('workspace.finish')}, make at least one successful ${modelToolName('workspace.commit')} call so the user can see the final chat message.`);
    }
    lines.push(`Do not answer directly without finishing through ${modelToolName('workspace.finish')}.`);

    return lines.join('\n');
}

function materializeDefaultProfilePrompt(profile) {
    profile.instructions = profile.instructions || {};
    if (profile.id === DEFAULT_PROFILE_ID && !String(profile.instructions.agentSystemPrompt || '').trim()) {
        profile.instructions.agentSystemPrompt = buildDefaultAgentSystemPrompt(profile);
    }
}

export function defaultProfile(id = DEFAULT_PROFILE_ID) {
    const profileId = normalizeProfileId(id) || DEFAULT_PROFILE_ID;
    const profile = {
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
            agentSystemPrompt: profileId === DEFAULT_PROFILE_ID ? buildDefaultAgentSystemPrompt() : null,
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
    materializeDefaultProfilePrompt(profile);
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
    materializeDefaultProfilePrompt(draft);
    draft.skills.visibleCsv = joinCsv(draft.skills.visible);
    draft.skills.denyCsv = joinCsv(draft.skills.deny);
    return draft;
}
