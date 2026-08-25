import { buildEventDetailTargets } from './run-event-detail-targets';
import {
    HANDOFF_EVENT_META,
    modelTurnNarration,
    presentRunEvent,
} from './run-event-presentation';
import {
    eventBelongsToInvocation,
    isRootInvocation,
    normalizeInvocationId,
    TRANSFER_CONTROL_CONTINUATION,
} from './run-invocation-projector';
import type {
    TimelineDelegationEdge,
    TimelineDetailTarget,
    TimelineItem,
} from './RunTimelineContract';

type RunEventPayload = Record<string, unknown>;
type TimelinePresenterOptions = {
    invocationId?: string;
    foregroundInvocationIds?: readonly string[];
    delegationEdges?: readonly TimelineDelegationEdge[];
};
type ShowEventOptions = {
    invocationId: string | null;
    foregroundInvocationIds: ReadonlySet<string> | null;
    acceptedHandoffInvocationIds: ReadonlySet<string>;
    projectedHandoffInvocationIds: ReadonlySet<string>;
};
type TransferControlEdge = {
    taskId: string;
    sourceInvocationId: string;
    targetInvocationId: string;
    targetProfileId: string;
    workspaceKey: string;
    status: string;
};

const DISPLAY_EVENT_TYPES: ReadonlySet<string> = new Set([
    'agent_delegate_started',
    'agent_handoff_accepted',
    'agent_invocation_started',
    'agent_invocation_completed',
    'agent_invocation_failed',
    'agent_invocation_cancelled',
    'agent_task_started',
    'agent_task_completed',
    'agent_task_failed',
    'agent_task_cancelled',
    'context_assembled',
    'task_return_completed',
    'tool_call_requested',
    'tool_call_completed',
    'tool_call_failed',
    'workspace_file_written',
    'direct_output_captured',
    'workspace_patch_applied',
    'chat_commit_requested',
    'chat_commit_completed',
    'chat_commit_failed',
    'persistent_changes_committed',
    'drift_recovery_attempted',
    'user_guidance_submitted',
    'user_guidance_applied',
    'user_guidance_discarded',
    'run_completed',
    'run_partial_success',
    'run_cancelled',
    'run_failed',
]);

const SIDE_EFFECT_TOOL_COMPLETIONS: ReadonlySet<string> = new Set([
    'builtin:agent.delegate',
    'builtin:agent.handoff',
    'builtin:task.return',
    'builtin:workspace.write_file',
    'builtin:workspace.apply_patch',
    'builtin:workspace.commit',
    'builtin:workspace.finish',
]);

export const TERMINAL_EVENT_TYPES = Object.freeze([
    'run_completed',
    'run_partial_success',
    'run_cancelled',
    'run_failed',
]);

export { buildEventDetailTargets, presentRunEvent };

export function isDisplayableRunEvent(event: TauriTavernAgentRunEvent): boolean {
    if (!DISPLAY_EVENT_TYPES.has(event.type)) return false;
    if (event.type !== 'context_assembled') return true;
    const payload = plainObject(event.payload) ? event.payload : {};
    return Array.isArray(payload.toolDiagnostics) && payload.toolDiagnostics.length > 0;
}

export function hasModelTurnNarration(event: TauriTavernAgentRunEvent): boolean {
    return event.type === 'model_completed' && Boolean(modelTurnNarration(event.payload));
}

export function timelineItemsFromEvents(
    events: readonly TauriTavernAgentRunEvent[],
    options: TimelinePresenterOptions = {},
): TimelineItem[] {
    const completedToolCalls = new Set<string>();
    const resolvedCommits = new Set<string>();
    const acceptedHandoffInvocationIds = new Set<string>();
    const invocationId = options.invocationId == null ? null : normalizeInvocationId(options.invocationId);
    const foregroundInvocationIds = normalizeForegroundInvocationIds(options.foregroundInvocationIds);
    const transferControlEdges = normalizeTransferControlEdges(options.delegationEdges);
    const projectedHandoffInvocationIds = new Set(
        transferControlEdges.map(edge => normalizeInvocationId(edge.targetInvocationId)),
    );

    for (const event of events) {
        const payload = plainObject(event.payload) ? event.payload : {};
        if (event.type === 'tool_call_completed' || event.type === 'tool_call_failed') {
            const callId = stringValue(payload.callId).trim();
            if (callId) completedToolCalls.add(callId);
        }
        if (event.type === 'chat_commit_completed' || event.type === 'chat_commit_failed') {
            const commitId = stringValue(payload.commitId).trim();
            if (commitId) resolvedCommits.add(commitId);
        }
        if (event.type === 'agent_handoff_accepted') {
            const newInvocationId = stringValue(payload.newInvocationId).trim();
            if (newInvocationId) acceptedHandoffInvocationIds.add(normalizeInvocationId(newInvocationId));
        }
    }

    const showOptions: ShowEventOptions = {
        invocationId,
        foregroundInvocationIds,
        acceptedHandoffInvocationIds,
        projectedHandoffInvocationIds,
    };
    const items = events
        .filter(event => shouldShowEvent(event, completedToolCalls, resolvedCommits, showOptions))
        .map(event => presentRunEvent(event, events));
    return insertProjectedHandoffBoundaries(items, transferControlEdges, acceptedHandoffInvocationIds);
}

function shouldShowEvent(
    event: TauriTavernAgentRunEvent,
    completedToolCalls: ReadonlySet<string>,
    resolvedCommits: ReadonlySet<string>,
    options: ShowEventOptions,
): boolean {
    if (event.type === 'model_completed') {
        if (!hasModelTurnNarration(event)) return false;
    } else if (!isDisplayableRunEvent(event)) {
        return false;
    }

    if (options.foregroundInvocationIds) {
        if (!eventBelongsToForegroundChain(event, options.foregroundInvocationIds)) return false;
        if (event.type.startsWith('agent_task_') || event.type === 'task_return_completed') return false;
        if (event.type.startsWith('agent_invocation_')) {
            const payload = plainObject(event.payload) ? event.payload : {};
            const invocationId = normalizeInvocationId(payload.invocationId);
            const kind = stringValue(payload.kind).trim();
            if (isRootInvocation(invocationId) || kind !== 'handoff') return false;
            if (event.type === 'agent_invocation_started') {
                return !options.acceptedHandoffInvocationIds.has(invocationId)
                    && !options.projectedHandoffInvocationIds.has(invocationId);
            }
            return event.type === 'agent_invocation_failed' || event.type === 'agent_invocation_cancelled';
        }
    }

    if (options.invocationId && !eventBelongsToInvocation(event, options.invocationId)) return false;
    if (options.invocationId && isRootInvocation(options.invocationId)
        && (event.type.startsWith('agent_task_') || event.type.startsWith('agent_invocation_'))) return false;

    const payload = plainObject(event.payload) ? event.payload : {};
    if (event.type === 'tool_call_requested') {
        const callId = stringValue(payload.callId).trim();
        return !callId || !completedToolCalls.has(callId);
    }
    if (event.type === 'tool_call_completed') {
        return !SIDE_EFFECT_TOOL_COMPLETIONS.has(stringValue(payload.toolId));
    }
    if (event.type === 'chat_commit_requested') {
        const commitId = stringValue(payload.commitId).trim();
        return !commitId || !resolvedCommits.has(commitId);
    }
    return true;
}

function normalizeForegroundInvocationIds(values?: readonly string[]): ReadonlySet<string> | null {
    if (!values) return null;
    const set = new Set(values.map(normalizeInvocationId));
    return set.size > 0 ? set : null;
}

function normalizeTransferControlEdges(values?: readonly TimelineDelegationEdge[]): TransferControlEdge[] {
    if (!values) return [];
    return values
        .filter(edge => edge.continuation === TRANSFER_CONTROL_CONTINUATION)
        .map(edge => ({
            taskId: edge.taskId.trim(),
            sourceInvocationId: normalizeInvocationId(edge.sourceInvocationId),
            targetInvocationId: normalizeInvocationId(edge.targetInvocationId),
            targetProfileId: edge.targetProfileId.trim(),
            workspaceKey: edge.workspaceKey.trim(),
            status: edge.status.trim(),
        }))
        .filter(edge => !isRootInvocation(edge.targetInvocationId));
}

function insertProjectedHandoffBoundaries(
    items: TimelineItem[],
    transferControlEdges: readonly TransferControlEdge[],
    acceptedHandoffInvocationIds: ReadonlySet<string>,
): TimelineItem[] {
    if (transferControlEdges.length === 0 || items.length === 0) return items;
    const next = [...items];
    for (const edge of transferControlEdges) {
        const invocationId = normalizeInvocationId(edge.targetInvocationId);
        if (acceptedHandoffInvocationIds.has(invocationId)) continue;
        const insertAt = next.findIndex(item => itemBelongsToInvocation(item, invocationId));
        if (insertAt < 0) continue;
        const anchor = next[insertAt];
        if (anchor) next.splice(insertAt, 0, projectedHandoffBoundary(edge, anchor));
    }
    return next;
}

function projectedHandoffBoundary(edge: TransferControlEdge, anchor: TimelineItem): TimelineItem {
    return {
        id: `handoff-boundary:${edge.taskId || edge.targetInvocationId}`,
        seq: anchor.seq - 0.001,
        runId: anchor.runId,
        type: 'agent_handoff_boundary',
        level: 'info',
        timestamp: anchor.timestamp,
        icon: HANDOFF_EVENT_META.icon,
        tone: HANDOFF_EVENT_META.tone,
        kind: 'handoff',
        titleKey: HANDOFF_EVENT_META.titleKey,
        titleParams: { agent: edge.targetProfileId || edge.targetInvocationId },
        summary: [edge.sourceInvocationId, edge.workspaceKey].filter(Boolean).join(' | '),
        detailTargets: [handoffDetailTarget(edge)],
    };
}

function handoffDetailTarget(edge: TransferControlEdge): TimelineDetailTarget {
    return {
        type: 'handoff',
        labelKey: 'timelineHandoff',
        taskId: edge.taskId,
        sourceInvocationId: edge.sourceInvocationId,
        newInvocationId: edge.targetInvocationId,
        targetProfileId: edge.targetProfileId,
        workspaceKey: edge.workspaceKey,
        status: edge.status,
    };
}

function itemBelongsToInvocation(item: TimelineItem, invocationId: string): boolean {
    const payload = plainObject(item.rawEvent?.payload) ? item.rawEvent.payload : {};
    return normalizeInvocationId(payload.invocationId) === invocationId;
}

function eventBelongsToForegroundChain(
    event: TauriTavernAgentRunEvent,
    foregroundInvocationIds: ReadonlySet<string>,
): boolean {
    const payload = plainObject(event.payload) ? event.payload : {};
    if (event.type.startsWith('run_')) return true;
    if (event.type === 'agent_handoff_accepted') {
        return foregroundInvocationIds.has(normalizeInvocationId(payload.sourceInvocationId))
            || foregroundInvocationIds.has(normalizeInvocationId(payload.newInvocationId));
    }
    if (event.type === 'agent_delegate_started') {
        return foregroundInvocationIds.has(normalizeInvocationId(payload.parentInvocationId));
    }
    if (event.type.startsWith('agent_task_') || event.type === 'task_return_completed') return false;
    return foregroundInvocationIds.has(normalizeInvocationId(payload.invocationId));
}

function stringValue(value: unknown): string {
    return typeof value === 'string' ? value : '';
}

function plainObject(value: unknown): value is RunEventPayload {
    return Boolean(value) && typeof value === 'object' && !Array.isArray(value);
}
