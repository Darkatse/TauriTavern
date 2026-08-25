import type { AgentSystemTr } from './i18n.js';
import type { SubAgentTask, TimelineItem, TimelinePanelStatus } from './RunTimelineContract';
import { isActiveTaskStatus } from './run-invocation-projector.js';

export function timelineItemTitle(item: TimelineItem, tr: AgentSystemTr): string {
    return tr(item.titleKey, item.titleParams);
}

export function timelineItemShortLabel(item: TimelineItem, tr: AgentSystemTr): string {
    const labels: Record<string, string> = {
        read: 'timelineOpRead',
        search: 'timelineOpSearch',
        list: 'timelineOpList',
        write: 'timelineOpWrite',
        patch: 'timelineOpPatch',
        commit: 'timelineOpCommit',
        persist: 'timelineOpPersist',
        done: 'timelineOpDone',
        fail: 'timelineOpFail',
        cancel: 'timelineOpCancel',
        model: 'timelineOpModel',
        narration: 'timelineOpNarration',
        handoff: 'timelineOpHandoff',
        subagent: 'timelineOpSubAgent',
        guidance: 'timelineOpGuidance',
    };
    const label = labels[item.kind];
    if (label) return tr(label);

    if (item.type === 'workspace_file_written') return tr('timelineOpWrite');
    if (item.type === 'workspace_patch_applied') return tr('timelineOpPatch');
    if (item.type === 'chat_commit_completed' || item.type === 'chat_commit_requested') return tr('timelineOpCommit');
    if (item.type === 'persistent_changes_committed') return tr('timelineOpPersist');
    if (item.type === 'run_completed') return tr('timelineOpDone');
    if (item.type === 'run_partial_success') return tr('timelineOpPartial');
    if (item.type === 'run_failed' || item.type === 'tool_call_failed' || item.type === 'chat_commit_failed') {
        return tr('timelineOpFail');
    }
    if (item.type === 'run_cancelled') return tr('timelineOpCancel');

    const payload = plainObject(item.rawEvent.payload) ? item.rawEvent.payload : {};
    const tool = stringValue(payload.name) || stringValue(item.titleParams.tool);
    if (tool.includes('read')) return tr('timelineOpRead');
    if (tool.includes('search')) return tr('timelineOpSearch');
    if (tool.includes('list')) return tr('timelineOpList');
    return tr('timelineOpTool');
}

export function timelineItemTime(item: TimelineItem): string {
    if (!item.timestamp) return '';
    const date = new Date(item.timestamp);
    return Number.isNaN(date.getTime())
        ? ''
        : date.toLocaleTimeString([], { hour: '2-digit', minute: '2-digit', second: '2-digit' });
}

export function shortRunId(runId: string): string {
    return runId.length <= 14 ? runId : `${runId.slice(0, 10)}...`;
}

export function subAgentStatusLabel(status: string, tr: AgentSystemTr): string {
    const labels: Record<string, string> = {
        queued: 'timelineStatusQueued',
        running: 'timelineStatusRunning',
        completed: 'timelineStatusCompleted',
        failed: 'timelineStatusFailed',
        cancelled: 'timelineStatusCancelled',
    };
    const label = labels[status];
    return label ? tr(label) : status;
}

export function runTimelineHeading(
    terminalType: string,
    isRunning: boolean,
    hasRun: boolean,
    tr: AgentSystemTr,
): { status: TimelinePanelStatus; title: string } {
    if (isRunning) return { status: 'running', title: tr('timelineRunning') };
    switch (terminalType) {
        case 'run_failed':
            return { status: 'failed', title: tr('timelineFailed') };
        case 'run_cancelled':
            return { status: 'cancelled', title: tr('timelineCancelled') };
        case 'run_partial_success':
            return { status: 'partial', title: tr('timelinePartialSuccess') };
        case 'run_completed':
            return { status: 'completed', title: tr('timelineCompleted') };
        default:
            return { status: hasRun ? 'ready' : 'idle', title: tr('timelineReady') };
    }
}

export function subAgentTrayTitle(tasks: readonly SubAgentTask[], tr: AgentSystemTr): string {
    const running = tasks.filter(task => isActiveTaskStatus(task.status)).length;
    if (running > 0) return tr('timelineSubAgentsRunning', { count: running });
    const failed = tasks.filter(task => task.status === 'failed').length;
    if (failed > 0) return tr('timelineSubAgentsFailed', { count: failed });
    const terminal = tasks.filter(task => ['completed', 'failed', 'cancelled'].includes(task.status)).length;
    return tr('timelineSubAgentsCompleted', { count: terminal });
}

export function subAgentTaskStyle(task: SubAgentTask): { '--ttas-subagent-color': string } {
    return { '--ttas-subagent-color': task.color };
}

export function subAgentTaskTone(task: SubAgentTask): string {
    if (task.status === 'failed') return 'failed';
    if (task.status === 'cancelled') return 'cancelled';
    if (task.status === 'completed') return 'completed';
    return isActiveTaskStatus(task.status) ? 'running' : 'queued';
}

function stringValue(value: unknown): string {
    return typeof value === 'string' ? value : '';
}

function plainObject(value: unknown): value is Record<string, unknown> {
    return Boolean(value) && typeof value === 'object' && !Array.isArray(value);
}
