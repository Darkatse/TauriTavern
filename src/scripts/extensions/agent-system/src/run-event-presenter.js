import { displayToolName } from './run-tool-labels.js';
import { eventBelongsToInvocation, isRootInvocation, normalizeInvocationId } from './run-invocation-projector.js';
import { textMetricFields, textMetricsSummary } from './run-text-metrics.js';
import { presentAgentRunFailure } from '../../../tauritavern/agent/agent-error-presenter.js';

const DISPLAY_EVENT_TYPES = new Set([
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
    'run_completed',
    'run_partial_success',
    'run_cancelled',
    'run_failed',
]);

export const TERMINAL_EVENT_TYPES = Object.freeze(['run_completed', 'run_partial_success', 'run_cancelled', 'run_failed']);

const SIDE_EFFECT_TOOL_COMPLETIONS = new Set([
    'agent.delegate',
    'agent.handoff',
    'task.return',
    'workspace.write_file',
    'workspace.apply_patch',
    'workspace.commit',
    'workspace.finish',
]);

const SIDE_EFFECT_TOOL_BY_EVENT_TYPE = Object.freeze({
    workspace_file_written: 'workspace.write_file',
    workspace_patch_applied: 'workspace.apply_patch',
    chat_commit_requested: 'workspace.commit',
    chat_commit_completed: 'workspace.commit',
    chat_commit_failed: 'workspace.commit',
    persistent_changes_committed: 'workspace.finish',
    run_completed: 'workspace.finish',
});

const EVENT_META = Object.freeze({
    agent_delegate_started: { icon: 'fa-diagram-project', tone: 'active', kind: 'subagent', titleKey: 'timelineEventSubAgentStarted' },
    agent_handoff_accepted: { icon: 'fa-arrow-right-arrow-left', tone: 'active', kind: 'handoff', titleKey: 'timelineEventHandoffAccepted' },
    agent_invocation_started: { icon: 'fa-circle-play', tone: 'active', kind: 'subagent', titleKey: 'timelineEventInvocationStarted' },
    agent_invocation_completed: { icon: 'fa-circle-check', tone: 'success', kind: 'subagent', titleKey: 'timelineEventInvocationCompleted' },
    agent_invocation_failed: { icon: 'fa-circle-exclamation', tone: 'error', kind: 'subagent', titleKey: 'timelineEventInvocationFailed' },
    agent_invocation_cancelled: { icon: 'fa-ban', tone: 'warn', kind: 'subagent', titleKey: 'timelineEventInvocationCancelled' },
    agent_task_started: { icon: 'fa-person-running', tone: 'active', kind: 'subagent', titleKey: 'timelineEventSubAgentTaskStarted' },
    agent_task_completed: { icon: 'fa-circle-check', tone: 'success', kind: 'subagent', titleKey: 'timelineEventSubAgentTaskCompleted' },
    agent_task_failed: { icon: 'fa-triangle-exclamation', tone: 'error', kind: 'subagent', titleKey: 'timelineEventSubAgentTaskFailed' },
    agent_task_cancelled: { icon: 'fa-ban', tone: 'warn', kind: 'subagent', titleKey: 'timelineEventSubAgentTaskCancelled' },
    task_return_completed: { icon: 'fa-reply', tone: 'success', kind: 'subagent', titleKey: 'timelineEventTaskReturned' },
    tool_call_requested: { icon: 'fa-screwdriver-wrench', tone: 'active', kind: 'tool', titleKey: 'timelineEventToolRequested' },
    tool_call_completed: { icon: 'fa-check', tone: 'success', kind: 'tool', titleKey: 'timelineEventToolCompleted' },
    tool_call_failed: { icon: 'fa-triangle-exclamation', tone: 'warn', kind: 'fail', titleKey: 'timelineEventToolFailed' },
    workspace_file_written: { icon: 'fa-file-lines', tone: 'success', kind: 'write', titleKey: 'timelineEventFileWritten' },
    direct_output_captured: { icon: 'fa-file-lines', tone: 'warn', kind: 'recover', titleKey: 'timelineEventDirectOutputCaptured' },
    workspace_patch_applied: { icon: 'fa-code-commit', tone: 'success', kind: 'patch', titleKey: 'timelineEventPatchApplied' },
    chat_commit_requested: { icon: 'fa-message', tone: 'active', kind: 'commit', titleKey: 'timelineEventCommitRequested' },
    chat_commit_completed: { icon: 'fa-circle-check', tone: 'success', kind: 'commit', titleKey: 'timelineEventCommitCompleted' },
    chat_commit_failed: { icon: 'fa-circle-exclamation', tone: 'error', kind: 'fail', titleKey: 'timelineEventCommitFailed' },
    persistent_changes_committed: { icon: 'fa-database', tone: 'success', kind: 'persist', titleKey: 'timelineEventPersistentCommitted' },
    drift_recovery_attempted: { icon: 'fa-arrows-rotate', tone: 'warn', kind: 'recover', titleKey: 'timelineEventDriftRecoveryAttempted' },
    run_completed: { icon: 'fa-circle-check', tone: 'success', kind: 'done', titleKey: 'timelineEventRunCompleted' },
    run_partial_success: { icon: 'fa-circle-exclamation', tone: 'warn', kind: 'partial', titleKey: 'timelineEventRunPartialSuccess' },
    run_cancelled: { icon: 'fa-ban', tone: 'warn', kind: 'cancel', titleKey: 'timelineEventRunCancelled' },
    run_failed: { icon: 'fa-circle-xmark', tone: 'error', kind: 'fail', titleKey: 'timelineEventRunFailed' },
});

export function isDisplayableRunEvent(event) {
    return DISPLAY_EVENT_TYPES.has(String(event?.type || ''));
}

export function timelineItemsFromEvents(events, options = {}) {
    const completedToolCalls = new Set();
    const resolvedCommits = new Set();
    const acceptedHandoffInvocationIds = new Set();
    const invocationId = options.invocationId == null ? null : normalizeInvocationId(options.invocationId);
    const foregroundInvocationIds = normalizeForegroundInvocationIds(options.foregroundInvocationIds);
    const handoffEdges = normalizeHandoffEdges(options.handoffEdges);
    const projectedHandoffInvocationIds = new Set(handoffEdges.map((edge) => normalizeInvocationId(edge.newInvocationId)));

    for (const event of events) {
        if (event?.type === 'tool_call_completed' || event?.type === 'tool_call_failed') {
            const callId = String(event?.payload?.callId || '').trim();
            if (callId) {
                completedToolCalls.add(callId);
            }
        }
        if (event?.type === 'chat_commit_completed' || event?.type === 'chat_commit_failed') {
            const commitId = String(event?.payload?.commitId || '').trim();
            if (commitId) {
                resolvedCommits.add(commitId);
            }
        }
        if (event?.type === 'agent_handoff_accepted') {
            const payload = plainObject(event?.payload) ? event.payload : {};
            const newInvocationId = String(payload.newInvocationId || '').trim();
            if (newInvocationId) {
                acceptedHandoffInvocationIds.add(normalizeInvocationId(newInvocationId));
            }
        }
    }

    const items = events
        .filter((event) => shouldShowEvent(event, completedToolCalls, resolvedCommits, {
            ...options,
            invocationId,
            foregroundInvocationIds,
            acceptedHandoffInvocationIds,
            projectedHandoffInvocationIds,
        }))
        .map((event) => presentRunEvent(event, events));
    return insertProjectedHandoffBoundaries(items, handoffEdges, acceptedHandoffInvocationIds);
}

export function presentRunEvent(event, allEvents = []) {
    const type = String(event?.type || '');
    const payload = plainObject(event?.payload) ? event.payload : {};
    const meta = EVENT_META[type] || {
        icon: 'fa-circle',
        tone: event?.level === 'error' ? 'error' : 'info',
        titleKey: 'timelineEventGeneric',
    };

    return {
        id: String(event?.id || `${event?.runId || 'run'}:${event?.seq || type}`),
        seq: Number(event?.seq || 0),
        runId: String(event?.runId || ''),
        type,
        level: String(event?.level || 'info'),
        timestamp: String(event?.timestamp || ''),
        icon: meta.icon,
        tone: event?.level === 'error' ? 'error' : meta.tone,
        kind: eventKind(type, payload, meta.kind),
        titleKey: meta.titleKey,
        titleParams: eventTitleParams(type, payload),
        summary: eventSummary(type, payload, allEvents),
        rawEvent: event,
    };
}

export function buildEventDetailTargets(item, allEvents) {
    if (Array.isArray(item?.detailTargets)) {
        return item.detailTargets;
    }

    const event = item?.rawEvent;
    const payload = plainObject(event?.payload) ? event.payload : {};
    const targets = [];
    const seenPaths = new Set();
    const seenReasoningRounds = new Set();

    const addFile = (labelKey, path, metricsSource = null) => {
        const normalized = String(path || '').trim();
        if (!normalized || seenPaths.has(normalized)) {
            return;
        }
        seenPaths.add(normalized);
        targets.push({
            type: 'file',
            labelKey,
            path: normalized,
            ...textMetricFields(metricsSource),
        });
    };
    const addModelReasoning = (round, invocationId) => {
        const normalized = Number(round);
        if (!Number.isInteger(normalized) || normalized <= 0) {
            return;
        }
        const normalizedInvocationId = normalizeInvocationId(invocationId);
        if (!modelTurnHasReasoning(allEvents, normalized, normalizedInvocationId)) {
            return;
        }
        if (seenReasoningRounds.has(normalized)) {
            return;
        }
        seenReasoningRounds.add(normalized);
        targets.push({
            type: 'modelReasoning',
            labelKey: 'timelineReasoning',
            round: normalized,
            ...invocationTargetFields(normalizedInvocationId),
        });
    };

    addModelReasoning(payload.round, payload.invocationId);
    const associatedTurn = findAssociatedToolTurn(event, allEvents);
    addModelReasoning(associatedTurn?.round, associatedTurn?.invocationId);
    addFile('timelineArguments', payload.argumentsRef);

    if (event?.type === 'agent_delegate_started'
        || event?.type === 'agent_task_started'
        || event?.type === 'agent_task_completed'
        || event?.type === 'agent_task_failed'
        || event?.type === 'agent_task_cancelled'
        || event?.type === 'task_return_completed') {
        targets.push({
            type: 'subAgentTask',
            labelKey: 'timelineSubAgent',
            taskId: payload.taskId || '',
            childInvocationId: payload.childInvocationId || '',
            targetProfileId: payload.targetProfileId || '',
            workspaceKey: payload.workspaceKey || '',
            status: payload.status || '',
            resultRef: payload.resultRef || '',
            summaryRef: payload.summaryRef || '',
            error: payload.error || '',
        });
    }

    if (event?.type === 'agent_handoff_accepted') {
        targets.push({
            type: 'handoff',
            labelKey: 'timelineHandoff',
            taskId: payload.taskId || '',
            sourceInvocationId: payload.sourceInvocationId || '',
            newInvocationId: payload.newInvocationId || '',
            targetProfileId: payload.targetProfileId || '',
            workspaceKey: payload.workspaceKey || '',
            status: 'accepted',
        });
    }

    if (event?.type === 'tool_call_completed' || event?.type === 'tool_call_failed') {
        const resultPath = findToolResultPath(allEvents, payload.callId);
        addFile('timelineToolResult', resultPath);
    }

    if (event?.type === 'workspace_patch_applied') {
        targets.push(buildPatchDiffTarget(event, allEvents));
    }

    if (event?.type === 'run_failed' || event?.type === 'run_partial_success') {
        targets.push({ type: 'runFailure', labelKey: 'timelineErrorDetails', event });
    }

    if (event?.type === 'task_return_completed') {
        addFile('timelineSubAgentSummary', payload.summaryRef);
        addFile('timelineSubAgentResult', payload.resultRef);
    }

    if (event?.type === 'workspace_file_written'
        || event?.type === 'direct_output_captured'
        || event?.type === 'workspace_patch_applied'
        || event?.type === 'chat_commit_requested'
        || event?.type === 'chat_commit_completed') {
        addFile('timelineWorkspaceFile', payload.path, event.payload);
    }

    return targets;
}

function buildPatchDiffTarget(event, events) {
    const payload = plainObject(event?.payload) ? event.payload : {};
    const path = String(payload.path || '').trim();
    const completed = findSideEffectToolCompletion(events, event, 'workspace.apply_patch', path);
    const callId = String(completed?.payload?.callId || '').trim();
    const requested = callId ? findToolRequest(events, callId) : null;
    const requestPayload = plainObject(requested?.payload) ? requested.payload : {};
    const argumentsRef = String(requestPayload.argumentsRef || '').trim();

    return {
        type: 'patchDiff',
        labelKey: 'timelinePatchDiff',
        path,
        argumentsRef,
        replacements: payload.replacements,
        ...textMetricFields(payload),
        errorKey: path && argumentsRef ? '' : 'timelinePatchDiffSourceMissing',
        errorParams: { path },
    };
}

function shouldShowEvent(event, completedToolCalls, resolvedCommits, options = {}) {
    if (event?.type === 'model_completed') {
        return false;
    }
    if (!isDisplayableRunEvent(event)) {
        return false;
    }
    if (options.foregroundInvocationIds) {
        if (!eventBelongsToForegroundChain(event, options.foregroundInvocationIds)) {
            return false;
        }
        if (event.type.startsWith('agent_task_') || event.type === 'task_return_completed') {
            return false;
        }
        if (event.type.startsWith('agent_invocation_')) {
            const payload = plainObject(event?.payload) ? event.payload : {};
            const invocationId = normalizeInvocationId(payload.invocationId);
            const kind = String(payload.kind || '').trim();
            if (isRootInvocation(invocationId) || kind !== 'handoff') {
                return false;
            }
            if (event.type === 'agent_invocation_started') {
                return !options.acceptedHandoffInvocationIds?.has(invocationId)
                    && !options.projectedHandoffInvocationIds?.has(invocationId);
            }
            return event.type === 'agent_invocation_failed'
                || event.type === 'agent_invocation_cancelled';
        }
    }
    if (options.invocationId && !eventBelongsToInvocation(event, options.invocationId)) {
        return false;
    }
    if (options.invocationId && isRootInvocation(options.invocationId) && event.type.startsWith('agent_task_')) {
        return false;
    }
    if (options.invocationId && isRootInvocation(options.invocationId) && event.type.startsWith('agent_invocation_')) {
        return false;
    }

    const payload = plainObject(event?.payload) ? event.payload : {};
    if (event.type === 'tool_call_requested') {
        const callId = String(payload.callId || '').trim();
        return !callId || !completedToolCalls.has(callId);
    }
    if (event.type === 'tool_call_completed') {
        return !SIDE_EFFECT_TOOL_COMPLETIONS.has(String(payload.name || ''));
    }
    if (event.type === 'chat_commit_requested') {
        const commitId = String(payload.commitId || '').trim();
        return !commitId || !resolvedCommits.has(commitId);
    }
    return true;
}

function findToolResultPath(events, callId) {
    const normalized = String(callId || '').trim();
    if (!normalized) {
        return '';
    }

    const resultEvent = [...events]
        .reverse()
        .find((event) => event?.type === 'tool_result_stored'
            && String(event?.payload?.callId || '') === normalized);
    return resultEvent?.payload?.path || '';
}

function normalizeForegroundInvocationIds(values) {
    if (!Array.isArray(values)) {
        return null;
    }
    const set = new Set(values.map(normalizeInvocationId));
    return set.size > 0 ? set : null;
}

function normalizeHandoffEdges(values) {
    if (!Array.isArray(values)) {
        return [];
    }
    return values
        .filter(plainObject)
        .map((edge) => ({
            taskId: String(edge.taskId || '').trim(),
            sourceInvocationId: normalizeInvocationId(edge.sourceInvocationId),
            newInvocationId: normalizeInvocationId(edge.newInvocationId),
            targetProfileId: String(edge.targetProfileId || '').trim(),
            workspaceKey: String(edge.workspaceKey || '').trim(),
            status: String(edge.status || '').trim(),
        }))
        .filter((edge) => !isRootInvocation(edge.newInvocationId));
}

function insertProjectedHandoffBoundaries(items, handoffEdges, acceptedHandoffInvocationIds) {
    if (handoffEdges.length === 0 || items.length === 0) {
        return items;
    }
    const next = [...items];
    for (const edge of handoffEdges) {
        const invocationId = normalizeInvocationId(edge.newInvocationId);
        if (acceptedHandoffInvocationIds.has(invocationId)) {
            continue;
        }
        const insertAt = next.findIndex((item) => itemBelongsToInvocation(item, invocationId));
        if (insertAt < 0) {
            continue;
        }
        const anchor = next[insertAt];
        next.splice(insertAt, 0, projectedHandoffBoundary(edge, anchor));
    }
    return next;
}

function projectedHandoffBoundary(edge, anchor) {
    const seq = Number(anchor?.seq || 0) - 0.001;
    return {
        id: `handoff-boundary:${edge.taskId || edge.newInvocationId}`,
        seq,
        runId: String(anchor?.runId || ''),
        type: 'agent_handoff_boundary',
        level: 'info',
        timestamp: String(anchor?.timestamp || ''),
        icon: EVENT_META.agent_handoff_accepted.icon,
        tone: EVENT_META.agent_handoff_accepted.tone,
        kind: 'handoff',
        titleKey: EVENT_META.agent_handoff_accepted.titleKey,
        titleParams: { agent: edge.targetProfileId || edge.newInvocationId || '' },
        summary: [edge.sourceInvocationId, edge.workspaceKey].filter(Boolean).join(' | '),
        detailTargets: [handoffDetailTarget(edge)],
    };
}

function handoffDetailTarget(edge) {
    return {
        type: 'handoff',
        labelKey: 'timelineHandoff',
        taskId: edge.taskId,
        sourceInvocationId: edge.sourceInvocationId,
        newInvocationId: edge.newInvocationId,
        targetProfileId: edge.targetProfileId,
        workspaceKey: edge.workspaceKey,
        status: edge.status,
    };
}

function itemBelongsToInvocation(item, invocationId) {
    const payload = plainObject(item?.rawEvent?.payload) ? item.rawEvent.payload : {};
    return normalizeInvocationId(payload.invocationId) === invocationId;
}

function eventBelongsToForegroundChain(event, foregroundInvocationIds) {
    const payload = plainObject(event?.payload) ? event.payload : {};
    const type = String(event?.type || '');

    if (type.startsWith('run_')) {
        return true;
    }
    if (type === 'agent_handoff_accepted') {
        return foregroundInvocationIds.has(normalizeInvocationId(payload.sourceInvocationId))
            || foregroundInvocationIds.has(normalizeInvocationId(payload.newInvocationId));
    }
    if (type === 'agent_delegate_started') {
        return foregroundInvocationIds.has(normalizeInvocationId(payload.parentInvocationId));
    }
    if (type.startsWith('agent_task_') || type === 'task_return_completed') {
        return false;
    }
    if (type.startsWith('agent_invocation_')) {
        return foregroundInvocationIds.has(normalizeInvocationId(payload.invocationId));
    }

    return foregroundInvocationIds.has(normalizeInvocationId(payload.invocationId));
}

function findAssociatedToolTurn(event, events) {
    const payload = plainObject(event?.payload) ? event.payload : {};
    const callId = String(payload.callId || '').trim();
    if (callId) {
        return findToolEventTurn(events, callId);
    }

    const toolName = SIDE_EFFECT_TOOL_BY_EVENT_TYPE[event?.type];
    if (!toolName) {
        return null;
    }

    const path = String(payload.path || '').trim();
    const completed = findSideEffectToolCompletion(events, event, toolName, path);
    return completed
        ? {
            round: completed?.payload?.round,
            invocationId: completed?.payload?.invocationId,
        }
        : null;
}

function findToolEventTurn(events, callId) {
    const event = events.find((candidate) => {
        if (candidate?.type !== 'tool_call_requested'
            && candidate?.type !== 'tool_call_completed'
            && candidate?.type !== 'tool_call_failed') {
            return false;
        }
        return String(candidate?.payload?.callId || '') === callId;
    });
    return event
        ? {
            round: event?.payload?.round,
            invocationId: event?.payload?.invocationId,
        }
        : null;
}

function findSideEffectToolCompletion(events, sideEffectEvent, toolName, path) {
    const sideEffectSeq = Number(sideEffectEvent?.seq || 0);
    return [...events]
        .reverse()
        .find((event) => {
            if (event?.type !== 'tool_call_completed' || Number(event?.seq || 0) >= sideEffectSeq) {
                return false;
            }

            const payload = plainObject(event?.payload) ? event.payload : {};
            if (payload.name !== toolName) {
                return false;
            }

            return !path || (Array.isArray(payload.resourceRefs) && payload.resourceRefs.includes(path));
        });
}

function findToolRequest(events, callId) {
    return events.find((event) => event?.type === 'tool_call_requested'
        && String(event?.payload?.callId || '') === callId);
}

function modelTurnHasReasoning(events, round, invocationId) {
    const normalizedInvocationId = normalizeInvocationId(invocationId);
    return events.some((event) => {
        if (event?.type !== 'model_completed') {
            return false;
        }
        const payload = plainObject(event?.payload) ? event.payload : {};
        return Number(payload.round) === round
            && normalizeInvocationId(payload.invocationId) === normalizedInvocationId
            && (
                payload.hasReasoning === true
                || Number(payload.reasoningChars) > 0
                || Number(payload.reasoningWords) > 0
            );
    });
}

function eventTitleParams(type, payload) {
    switch (type) {
        case 'agent_handoff_accepted':
            return { agent: payload.targetProfileId || payload.newInvocationId || '' };
        case 'agent_delegate_started':
        case 'agent_task_started':
        case 'agent_task_completed':
        case 'agent_task_failed':
        case 'agent_task_cancelled':
            return { agent: payload.targetProfileId || payload.workspaceKey || payload.childInvocationId || '' };
        case 'agent_invocation_started':
        case 'agent_invocation_completed':
        case 'agent_invocation_failed':
        case 'agent_invocation_cancelled':
            return { agent: payload.profileId || payload.invocationId || '' };
        case 'task_return_completed':
            return { task: payload.taskId || '' };
        case 'tool_call_requested':
        case 'tool_call_completed':
        case 'tool_call_failed':
            return { tool: toolLabel(payload.name) };
        case 'workspace_file_written':
        case 'direct_output_captured':
        case 'workspace_patch_applied':
        case 'chat_commit_requested':
        case 'chat_commit_completed':
            return { path: payload.path || '' };
        case 'persistent_changes_committed':
            return { count: payload.changeCount ?? 0 };
        case 'drift_recovery_attempted':
            return { attempt: payload.attempt ?? 0, max: payload.maxAttempts ?? 0 };
        case 'run_partial_success':
            return { count: payload.preservedCommitCount ?? 0 };
        default:
            return {};
    }
}

function eventSummary(type, payload, allEvents) {
    switch (type) {
        case 'agent_handoff_accepted':
            return [payload.sourceInvocationId, payload.workspaceKey].filter(Boolean).join(' | ');
        case 'agent_delegate_started':
        case 'agent_task_started':
        case 'agent_task_completed':
        case 'agent_task_failed':
        case 'agent_task_cancelled':
            return [payload.status, payload.workspaceKey].filter(Boolean).join(' | ');
        case 'agent_invocation_started':
        case 'agent_invocation_completed':
        case 'agent_invocation_failed':
        case 'agent_invocation_cancelled':
            return [payload.status, payload.kind].filter(Boolean).join(' | ');
        case 'task_return_completed':
            return [payload.status, payload.summaryRef || payload.resultRef].filter(Boolean).join(' | ');
        case 'tool_call_requested':
            return payload.callId || '';
        case 'tool_call_completed':
            return textMetricsSummary(payload.displayMetrics)
                || textMetricsSummary(payload)
                || resourceSummary(payload.resourceRefs)
                || elapsedSummary(payload.elapsedMs);
        case 'tool_call_failed':
            return payload.message || payload.errorCode || '';
        case 'workspace_file_written':
        case 'direct_output_captured':
        case 'workspace_patch_applied':
            return fileSummary(payload);
        case 'chat_commit_requested':
            return commitSummary(payload);
        case 'chat_commit_completed':
            return commitCompletedSummary(payload, allEvents);
        case 'chat_commit_failed':
            return payload.message || '';
        case 'persistent_changes_committed':
            return Array.isArray(payload.changes) ? payload.changes.map((change) => change.path).filter(Boolean).join(', ') : '';
        case 'drift_recovery_attempted':
            return payload.reasonCode || '';
        case 'run_cancelled':
            return payload.message || '';
        case 'run_partial_success':
            return partialSuccessSummary(payload);
        case 'run_failed':
            return presentAgentRunFailure({ payload }).summary;
        default:
            return '';
    }
}

function eventKind(type, payload, fallback) {
    if (type === 'agent_handoff_accepted') {
        return 'handoff';
    }
    if (type === 'tool_call_requested' || type === 'tool_call_completed') {
        return toolKind(payload.name);
    }
    return fallback || 'event';
}

function toolKind(name) {
    const normalized = String(name || '');
    if (normalized.startsWith('agent.') || normalized === 'task.return') {
        return 'subagent';
    }
    if (normalized.includes('read')) {
        return 'read';
    }
    if (normalized.includes('search')) {
        return 'search';
    }
    if (normalized.includes('list')) {
        return 'list';
    }
    if (normalized === 'workspace.write_file') {
        return 'write';
    }
    if (normalized === 'workspace.apply_patch') {
        return 'patch';
    }
    if (normalized === 'workspace.commit') {
        return 'commit';
    }
    if (normalized === 'workspace.finish') {
        return 'done';
    }
    return 'tool';
}

function invocationTargetFields(invocationId) {
    const normalized = normalizeInvocationId(invocationId);
    return isRootInvocation(normalized) ? {} : { invocationId: normalized };
}

function fileSummary(payload) {
    const parts = [];
    const metrics = textMetricsSummary(payload);
    if (metrics) {
        parts.push(metrics);
    }
    if (payload.replacements != null) {
        parts.push(`${payload.replacements} replacements`);
    }
    return parts.join(' | ');
}

function commitSummary(payload) {
    const parts = [payload.mode, payload.reason, textMetricsSummary(payload)];
    return parts.filter(Boolean).join(' | ');
}

function commitCompletedSummary(payload, events) {
    const parts = [payload.messageId ? `message ${payload.messageId}` : payload.mode || ''];
    const requested = findCommitRequestedEvent(events, payload.commitId);
    parts.push(textMetricsSummary(payload) || textMetricsSummary(requested?.payload));
    return parts.filter(Boolean).join(' | ');
}

function findCommitRequestedEvent(events, commitId) {
    const normalized = String(commitId || '').trim();
    if (!normalized) {
        return null;
    }
    return events.find((event) => event?.type === 'chat_commit_requested'
        && String(event?.payload?.commitId || '') === normalized) || null;
}

function resourceSummary(resourceRefs) {
    if (!Array.isArray(resourceRefs) || resourceRefs.length === 0) {
        return '';
    }
    return resourceRefs.join(', ');
}

function elapsedSummary(value) {
    const elapsed = Number(value);
    if (!Number.isFinite(elapsed) || elapsed <= 0) {
        return '';
    }
    return `${Math.round(elapsed)} ms`;
}

function partialSuccessSummary(payload) {
    const count = Number(payload.preservedCommitCount);
    if (Number.isInteger(count) && count > 0) {
        return `${count} committed message${count === 1 ? '' : 's'} preserved`;
    }
    return payload.message || payload.code || '';
}

function toolLabel(name) {
    return displayToolName(name);
}

function plainObject(value) {
    return Boolean(value) && typeof value === 'object' && !Array.isArray(value);
}
