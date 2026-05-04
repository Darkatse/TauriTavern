export const MODULE_NAME = 'agent-system';
export const SETTINGS_KEY = 'settings';
export const DEFAULT_PROFILE_ID = 'default-writer';

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

export const DEFAULT_SETTINGS = Object.freeze({
    agentModeEnabled: false,
    selectedProfileId: DEFAULT_PROFILE_ID,
    activeTab: 'profiles',
});
