import { DEFAULT_AGENT_PROFILE_ID } from '../../../tauritavern/agent/agent-system-settings.js';

export const DEFAULT_PROFILE_ID = DEFAULT_AGENT_PROFILE_ID;

export const AGENT_SUBAGENT_TOOLS = Object.freeze([
    'builtin:agent.list',
    'builtin:agent.delegate',
    'builtin:agent.await',
]);

export const AGENT_HANDOFF_TOOLS = Object.freeze([
    'builtin:agent.list',
    'builtin:agent.handoff',
]);

export const AGENT_DELEGATION_TOOLS = Object.freeze([
    ...new Set([
        ...AGENT_SUBAGENT_TOOLS,
        ...AGENT_HANDOFF_TOOLS,
    ]),
]);

export const RUNTIME_ONLY_TOOLS = Object.freeze([
    'builtin:task.return',
]);

export const KNOWN_TOOLS = Object.freeze([
    'builtin:chat.search',
    'builtin:chat.read_messages',
    'builtin:worldinfo.read_activated',
    'builtin:skill.list',
    'builtin:skill.search',
    'builtin:skill.read',
    'builtin:workspace.list_files',
    'builtin:workspace.search_files',
    'builtin:workspace.read_file',
    'builtin:workspace.write_file',
    'builtin:workspace.apply_patch',
    'builtin:workspace.commit',
    'builtin:workspace.finish',
]);

export const WORKSPACE_ROOTS = Object.freeze(['output', 'scratch', 'plan', 'summaries', 'persist']);
