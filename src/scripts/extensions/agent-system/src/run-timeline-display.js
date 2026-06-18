import { translateAgentSystem as tr } from './i18n.js';
import { isActiveTaskStatus } from './run-invocation-projector.js';

export function timelineItemTitle(item) {
    return tr(item.titleKey, item.titleParams || {});
}

export function timelineItemShortLabel(item) {
    switch (String(item?.kind || '')) {
        case 'read':
            return tr('timelineOpRead');
        case 'search':
            return tr('timelineOpSearch');
        case 'list':
            return tr('timelineOpList');
        case 'write':
            return tr('timelineOpWrite');
        case 'patch':
            return tr('timelineOpPatch');
        case 'commit':
            return tr('timelineOpCommit');
        case 'persist':
            return tr('timelineOpPersist');
        case 'done':
            return tr('timelineOpDone');
        case 'fail':
            return tr('timelineOpFail');
        case 'cancel':
            return tr('timelineOpCancel');
        case 'model':
            return tr('timelineOpModel');
        case 'narration':
            return tr('timelineOpNarration');
        case 'handoff':
            return tr('timelineOpHandoff');
        case 'subagent':
            return tr('timelineOpSubAgent');
        case 'guidance':
            return tr('timelineOpGuidance');
        default:
            break;
    }

    const type = String(item?.type || '');
    if (type === 'workspace_file_written') {
        return tr('timelineOpWrite');
    }
    if (type === 'workspace_patch_applied') {
        return tr('timelineOpPatch');
    }
    if (type === 'chat_commit_completed' || type === 'chat_commit_requested') {
        return tr('timelineOpCommit');
    }
    if (type === 'persistent_changes_committed') {
        return tr('timelineOpPersist');
    }
    if (type === 'run_completed') {
        return tr('timelineOpDone');
    }
    if (type === 'run_partial_success') {
        return tr('timelineOpPartial');
    }
    if (type === 'run_failed' || type === 'tool_call_failed' || type === 'chat_commit_failed') {
        return tr('timelineOpFail');
    }
    if (type === 'run_cancelled') {
        return tr('timelineOpCancel');
    }

    const tool = String(item?.rawEvent?.payload?.name || item?.titleParams?.tool || '');
    if (tool.includes('read')) {
        return tr('timelineOpRead');
    }
    if (tool.includes('search')) {
        return tr('timelineOpSearch');
    }
    if (tool.includes('list')) {
        return tr('timelineOpList');
    }
    return tr('timelineOpTool');
}

export function timelineItemTime(item) {
    if (!item.timestamp) {
        return '';
    }
    const date = new Date(item.timestamp);
    if (Number.isNaN(date.getTime())) {
        return '';
    }
    return date.toLocaleTimeString([], { hour: '2-digit', minute: '2-digit', second: '2-digit' });
}

export function shortRunId(runId) {
    const value = String(runId || '');
    if (value.length <= 14) {
        return value;
    }
    return `${value.slice(0, 10)}...`;
}

export function subAgentStatusLabel(status) {
    switch (String(status || '')) {
        case 'queued':
            return tr('timelineStatusQueued');
        case 'running':
            return tr('timelineStatusRunning');
        case 'completed':
            return tr('timelineStatusCompleted');
        case 'failed':
            return tr('timelineStatusFailed');
        case 'cancelled':
            return tr('timelineStatusCancelled');
        default:
            return String(status || '');
    }
}

export function subAgentTaskStyle(task) {
    return {
        '--ttas-subagent-color': task.color,
    };
}

export function subAgentTaskTone(task) {
    if (task.status === 'failed') {
        return 'failed';
    }
    if (task.status === 'cancelled') {
        return 'cancelled';
    }
    if (task.status === 'completed') {
        return 'completed';
    }
    return isActiveTaskStatus(task.status) ? 'running' : 'queued';
}
