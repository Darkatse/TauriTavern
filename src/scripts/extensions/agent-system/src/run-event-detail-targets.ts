import type { AgentSystemMessageKey } from './i18n';
import {
    isRootInvocation,
    normalizeInvocationId,
} from './run-invocation-projector';
import { modelTurnNarration } from './run-event-presentation';
import { textMetricFields } from './run-text-metrics';
import type { TimelineDetailTarget, TimelineItem } from './RunTimelineContract';

type RunEventPayload = Record<string, unknown>;

const SIDE_EFFECT_TOOL_BY_EVENT_TYPE: Readonly<Record<string, string>> = Object.freeze({
    workspace_file_written: 'builtin:workspace.write_file',
    workspace_patch_applied: 'builtin:workspace.apply_patch',
    chat_commit_requested: 'builtin:workspace.commit',
    chat_commit_completed: 'builtin:workspace.commit',
    chat_commit_failed: 'builtin:workspace.commit',
    persistent_changes_committed: 'builtin:workspace.finish',
    run_completed: 'builtin:workspace.finish',
});

export function buildEventDetailTargets(
    item: TimelineItem,
    allEvents: readonly TauriTavernAgentRunEvent[],
): TimelineDetailTarget[] {
    if (Array.isArray(item.detailTargets)) {
        return item.detailTargets;
    }

    const event = item.rawEvent;
    if (!event) {
        return [];
    }
    const payload = plainObject(event.payload) ? event.payload : {};
    const targets: TimelineDetailTarget[] = [];
    const seenPaths = new Set<string>();
    const seenReasoningRounds = new Set<number>();

    const addFile = (labelKey: AgentSystemMessageKey, path: unknown, metricsSource: unknown = null): void => {
        const normalized = stringValue(path).trim();
        if (!normalized || seenPaths.has(normalized)) return;
        seenPaths.add(normalized);
        targets.push({
            type: 'file',
            labelKey,
            path: normalized,
            ...textMetricFields(metricsSource),
        });
    };
    const addModelReasoning = (round: unknown, invocationId: unknown): void => {
        const normalized = Number(round);
        if (!Number.isInteger(normalized) || normalized <= 0) return;
        const normalizedInvocationId = normalizeInvocationId(invocationId);
        if (!modelTurnHasReasoning(allEvents, normalized, normalizedInvocationId)) return;
        if (seenReasoningRounds.has(normalized)) return;
        seenReasoningRounds.add(normalized);
        targets.push({
            type: 'modelReasoning',
            labelKey: 'timelineReasoning',
            round: normalized,
            ...invocationTargetFields(normalizedInvocationId),
        });
    };
    const addModelNarration = (round: unknown, invocationId: unknown): void => {
        const normalized = Number(round);
        if (!Number.isInteger(normalized) || normalized <= 0 || !modelTurnNarration(payload)) return;
        const normalizedInvocationId = normalizeInvocationId(invocationId);
        targets.push({
            type: 'modelNarration',
            labelKey: 'timelineNarration',
            round: normalized,
            ...invocationTargetFields(normalizedInvocationId),
        });
    };

    addModelNarration(payload.round, payload.invocationId);
    addModelReasoning(payload.round, payload.invocationId);
    const associatedTurn = findAssociatedToolTurn(event, allEvents);
    addModelReasoning(associatedTurn?.round, associatedTurn?.invocationId);
    addFile('timelineArguments', payload.argumentsRef);

    if (isSubAgentTaskEvent(event.type)) {
        targets.push({
            type: 'subAgentTask',
            labelKey: 'timelineSubAgent',
            taskId: stringValue(payload.taskId),
            childInvocationId: stringValue(payload.childInvocationId),
            targetProfileId: stringValue(payload.targetProfileId),
            workspaceKey: stringValue(payload.workspaceKey),
            status: stringValue(payload.status),
            resultRef: stringValue(payload.resultRef),
            summaryRef: stringValue(payload.summaryRef),
            error: stringValue(payload.error),
        });
    }

    if (event.type === 'agent_handoff_accepted') {
        targets.push({
            type: 'handoff',
            labelKey: 'timelineHandoff',
            taskId: stringValue(payload.taskId),
            sourceInvocationId: stringValue(payload.sourceInvocationId),
            newInvocationId: stringValue(payload.newInvocationId),
            targetProfileId: stringValue(payload.targetProfileId),
            workspaceKey: stringValue(payload.workspaceKey),
            status: 'accepted',
        });
    }

    if (event.type === 'tool_call_completed' || event.type === 'tool_call_failed') {
        addFile('timelineToolResult', findToolResultPath(allEvents, payload.callId));
    }

    if (event.type === 'workspace_patch_applied') {
        targets.push(buildPatchDiffTarget(event, allEvents));
    }

    if (event.type === 'run_failed' || event.type === 'run_partial_success') {
        targets.push({ type: 'runFailure', labelKey: 'timelineErrorDetails', event });
    }

    if (event.type === 'task_return_completed') {
        addFile('timelineSubAgentSummary', payload.summaryRef);
        addFile('timelineSubAgentResult', payload.resultRef);
    }

    if (isWorkspaceFileEvent(event.type)) {
        addFile('timelineWorkspaceFile', payload.path, payload);
    }

    if (event.type === 'user_guidance_submitted'
        || event.type === 'user_guidance_applied'
        || event.type === 'user_guidance_discarded') {
        targets.push(buildGuidanceDetailTarget(payload));
    }

    return targets;
}

function buildGuidanceDetailTarget(payload: RunEventPayload): TimelineDetailTarget {
    const round = optionalNumber(payload.round);
    return {
        type: 'guidance',
        labelKey: 'timelineGuidance',
        guidanceIds: normalizeGuidanceIds(payload),
        clientGuidanceIds: normalizeClientGuidanceIds(payload),
        invocationId: stringValue(payload.invocationId),
        ...(round === undefined ? {} : { round }),
        status: stringValue(payload.status),
        reason: stringValue(payload.reason),
        text: stringValue(payload.text),
        preview: stringValue(payload.preview),
        ...textMetricFields(payload),
    };
}

function buildPatchDiffTarget(
    event: TauriTavernAgentRunEvent,
    events: readonly TauriTavernAgentRunEvent[],
): TimelineDetailTarget {
    const payload = plainObject(event.payload) ? event.payload : {};
    const path = stringValue(payload.path).trim();
    const completed = findSideEffectToolCompletion(events, event, 'builtin:workspace.apply_patch', path);
    const completedPayload = plainObject(completed?.payload) ? completed.payload : {};
    const callId = stringValue(completedPayload.callId).trim();
    const requested = callId ? findToolRequest(events, callId) : null;
    const requestPayload = plainObject(requested?.payload) ? requested.payload : {};
    const argumentsRef = stringValue(requestPayload.argumentsRef).trim();
    const replacements = optionalNumber(payload.replacements);

    return {
        type: 'patchDiff',
        labelKey: 'timelinePatchDiff',
        path,
        argumentsRef,
        ...(replacements === undefined ? {} : { replacements }),
        ...textMetricFields(payload),
        errorKey: path && argumentsRef ? '' : 'timelinePatchDiffSourceMissing',
        errorParams: { path },
    };
}

function findToolResultPath(events: readonly TauriTavernAgentRunEvent[], callId: unknown): string {
    const normalized = stringValue(callId).trim();
    if (!normalized) return '';
    const resultEvent = [...events].reverse().find((event) => {
        const payload = plainObject(event.payload) ? event.payload : {};
        return event.type === 'tool_result_stored' && stringValue(payload.callId) === normalized;
    });
    const payload = plainObject(resultEvent?.payload) ? resultEvent.payload : {};
    return stringValue(payload.path);
}

function findAssociatedToolTurn(
    event: TauriTavernAgentRunEvent,
    events: readonly TauriTavernAgentRunEvent[],
): { round: unknown; invocationId: unknown } | null {
    const payload = plainObject(event.payload) ? event.payload : {};
    const callId = stringValue(payload.callId).trim();
    if (callId) return findToolEventTurn(events, callId);

    const toolId = SIDE_EFFECT_TOOL_BY_EVENT_TYPE[event.type];
    if (!toolId) return null;
    const completed = findSideEffectToolCompletion(events, event, toolId, stringValue(payload.path).trim());
    const completedPayload = plainObject(completed?.payload) ? completed.payload : null;
    return completedPayload
        ? { round: completedPayload.round, invocationId: completedPayload.invocationId }
        : null;
}

function findToolEventTurn(
    events: readonly TauriTavernAgentRunEvent[],
    callId: string,
): { round: unknown; invocationId: unknown } | null {
    const event = events.find((candidate) => {
        if (candidate.type !== 'tool_call_requested'
            && candidate.type !== 'tool_call_completed'
            && candidate.type !== 'tool_call_failed') return false;
        const payload = plainObject(candidate.payload) ? candidate.payload : {};
        return stringValue(payload.callId) === callId;
    });
    const payload = plainObject(event?.payload) ? event.payload : null;
    return payload ? { round: payload.round, invocationId: payload.invocationId } : null;
}

function findSideEffectToolCompletion(
    events: readonly TauriTavernAgentRunEvent[],
    sideEffectEvent: TauriTavernAgentRunEvent,
    toolId: string,
    path: string,
): TauriTavernAgentRunEvent | undefined {
    return [...events].reverse().find((event) => {
        if (event.type !== 'tool_call_completed' || event.seq >= sideEffectEvent.seq) return false;
        const payload = plainObject(event.payload) ? event.payload : {};
        if (payload.toolId !== toolId) return false;
        return !path || (Array.isArray(payload.resourceRefs) && payload.resourceRefs.includes(path));
    });
}

function findToolRequest(
    events: readonly TauriTavernAgentRunEvent[],
    callId: string,
): TauriTavernAgentRunEvent | null {
    return events.find((event) => {
        const payload = plainObject(event.payload) ? event.payload : {};
        return event.type === 'tool_call_requested' && stringValue(payload.callId) === callId;
    }) ?? null;
}

function modelTurnHasReasoning(
    events: readonly TauriTavernAgentRunEvent[],
    round: number,
    invocationId: string,
): boolean {
    const normalizedInvocationId = normalizeInvocationId(invocationId);
    return events.some((event) => {
        if (event.type !== 'model_completed') return false;
        const payload = plainObject(event.payload) ? event.payload : {};
        return Number(payload.round) === round
            && normalizeInvocationId(payload.invocationId) === normalizedInvocationId
            && (payload.hasReasoning === true
                || Number(payload.reasoningChars) > 0
                || Number(payload.reasoningWords) > 0);
    });
}

function invocationTargetFields(invocationId: string): { invocationId?: string } {
    const normalized = normalizeInvocationId(invocationId);
    return isRootInvocation(normalized) ? {} : { invocationId: normalized };
}

function normalizeGuidanceIds(payload: RunEventPayload): string[] {
    const ids = normalizeStringArray(payload.guidanceIds);
    const guidanceId = stringValue(payload.guidanceId).trim();
    return guidanceId ? [guidanceId, ...ids] : ids;
}

function normalizeClientGuidanceIds(payload: RunEventPayload): string[] {
    const ids = normalizeStringArray(payload.clientGuidanceIds);
    const clientGuidanceId = stringValue(payload.clientGuidanceId).trim();
    return clientGuidanceId ? [clientGuidanceId, ...ids] : ids;
}

function normalizeStringArray(value: unknown): string[] {
    return Array.isArray(value)
        ? value.map(stringValue).map(item => item.trim()).filter(Boolean)
        : [];
}

function isSubAgentTaskEvent(type: string): boolean {
    return type === 'agent_delegate_started'
        || type === 'agent_task_started'
        || type === 'agent_task_completed'
        || type === 'agent_task_failed'
        || type === 'agent_task_cancelled'
        || type === 'task_return_completed';
}

function isWorkspaceFileEvent(type: string): boolean {
    return type === 'workspace_file_written'
        || type === 'direct_output_captured'
        || type === 'workspace_patch_applied'
        || type === 'chat_commit_requested'
        || type === 'chat_commit_completed';
}

function optionalNumber(value: unknown): number | undefined {
    const number = Number(value);
    return Number.isFinite(number) ? number : undefined;
}

function stringValue(value: unknown): string {
    return typeof value === 'string' ? value : '';
}

function plainObject(value: unknown): value is RunEventPayload {
    return Boolean(value) && typeof value === 'object' && !Array.isArray(value);
}
