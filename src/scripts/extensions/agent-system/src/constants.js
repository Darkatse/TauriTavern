import { DEFAULT_AGENT_PROFILE_ID } from '../../../tauritavern/agent/agent-system-settings.js';

export const DEFAULT_PROFILE_ID = DEFAULT_AGENT_PROFILE_ID;

export const AGENT_SUBAGENT_TOOLS = Object.freeze([
    'agent.list',
    'agent.delegate',
    'agent.await',
]);

export const AGENT_HANDOFF_TOOLS = Object.freeze([
    'agent.list',
    'agent.handoff',
]);

export const AGENT_DELEGATION_TOOLS = Object.freeze([
    ...new Set([
        ...AGENT_SUBAGENT_TOOLS,
        ...AGENT_HANDOFF_TOOLS,
    ]),
]);

export const RUNTIME_ONLY_TOOLS = Object.freeze([
    'task.return',
]);

export const KNOWN_TOOLS = Object.freeze([
    'chat.search',
    'chat.read_messages',
    'worldinfo.read_activated',
    'skill.list',
    'skill.search',
    'skill.read',
    'workspace.list_files',
    'workspace.search_files',
    'workspace.read_file',
    'workspace.write_file',
    'workspace.apply_patch',
    'workspace.commit',
    'workspace.finish',
]);

export const WORKSPACE_ROOTS = Object.freeze(['output', 'scratch', 'plan', 'summaries', 'persist']);
