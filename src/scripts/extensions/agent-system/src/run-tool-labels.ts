import { translateAgentSystem as tr, type AgentSystemMessageKey } from './i18n';

const TOOL_LABEL_KEYS: Readonly<Record<string, AgentSystemMessageKey>> = Object.freeze({
    'agent.list': 'timelineToolAgentList',
    'agent.delegate': 'timelineToolAgentDelegate',
    'agent.handoff': 'timelineToolAgentHandoff',
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

export function displayToolName(name: unknown): string {
    const normalized = typeof name === 'string' ? name.trim() : '';
    if (!normalized) {
        return tr('timelineToolGeneric');
    }

    const key = TOOL_LABEL_KEYS[normalized];
    return key ? tr(key) : readableUnknownToolName(normalized);
}

export function displayToolLabel(toolId: string): string {
    const name = nativeToolName(toolId);
    return toolId.startsWith('builtin:')
        ? displayToolName(name)
        : `${readableUnknownToolName(name)} [${toolId}]`;
}

export function nativeToolName(toolId: unknown): string {
    if (typeof toolId !== 'string') throw new TypeError('a canonical toolId is required');
    const separator = toolId.indexOf(':');
    if (separator <= 0 || separator === toolId.length - 1) {
        throw new TypeError('a canonical toolId is required');
    }
    return toolId.slice(separator + 1);
}

function readableUnknownToolName(name: string): string {
    return name
        .slice(name.lastIndexOf('.') + 1)
        .replace(/[_-]+/g, ' ')
        .trim()
        || tr('timelineToolGeneric');
}
