import { translateAgentSystem as tr } from './i18n.js';

const TOOL_LABEL_KEYS = Object.freeze({
    'agent.list': 'timelineToolAgentList',
    'agent.delegate': 'timelineToolAgentDelegate',
    'agent.await': 'timelineToolAgentAwait',
    'task.return': 'timelineToolTaskReturn',
    'chat.search': 'timelineToolChatSearch',
    'chat.read_messages': 'timelineToolChatReadMessages',
    'dice.roll': 'timelineToolDiceRoll',
    'worldinfo.read_activated': 'timelineToolWorldInfoReadActivated',
    'skill.list': 'timelineToolSkillList',
    'skill.search': 'timelineToolSkillSearch',
    'skill.read': 'timelineToolSkillRead',
    'workspace.list_files': 'timelineToolWorkspaceListFiles',
    'workspace.search_files': 'timelineToolWorkspaceSearchFiles',
    'workspace.read_file': 'timelineToolWorkspaceReadFile',
    'workspace.write_file': 'timelineToolWorkspaceWriteFile',
    'workspace.apply_patch': 'timelineToolWorkspaceApplyPatch',
    'workspace.commit': 'timelineToolWorkspaceCommit',
    'workspace.finish': 'timelineToolWorkspaceFinish',
});

export function displayToolName(name) {
    const normalized = String(name || '').trim();
    if (!normalized) {
        return tr('timelineToolGeneric');
    }

    const key = TOOL_LABEL_KEYS[normalized];
    return key ? tr(key) : readableUnknownToolName(normalized);
}

function readableUnknownToolName(name) {
    return name
        .split('.')
        .at(-1)
        .replace(/[_-]+/g, ' ')
        .trim()
        || tr('timelineToolGeneric');
}
