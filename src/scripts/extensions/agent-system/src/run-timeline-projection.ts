import { normalizeInvocationId } from './run-invocation-projector';
import type {
    TimelineDelegationEdge,
    TimelineProjection,
    TimelineProjectionInvocation,
} from './RunTimelineContract';

export function emptyTimelineProjection(): TimelineProjection {
    return { foregroundInvocationIds: [], invocations: [], delegationEdges: [] };
}

export function normalizeTimelineProjection(value: unknown): TimelineProjection {
    if (!plainObject(value)) {
        throw new Error('agent.timeline_projection_invalid: readEvents.timelineProjection must be an object');
    }
    if (!Array.isArray(value.foregroundInvocationIds)) {
        throw new Error('agent.timeline_projection_invalid: foregroundInvocationIds must be an array');
    }
    if (!Array.isArray(value.invocations)) {
        throw new Error('agent.timeline_projection_invalid: invocations must be an array');
    }
    if (!Array.isArray(value.delegationEdges)) {
        throw new Error('agent.timeline_projection_invalid: delegationEdges must be an array');
    }
    return {
        foregroundInvocationIds: value.foregroundInvocationIds.map(normalizeInvocationId),
        invocations: value.invocations.map(normalizeProjectionInvocation),
        delegationEdges: value.delegationEdges.map(normalizeProjectionDelegationEdge),
    };
}

export function isTimelineProjectionStructuralEvent(type: string): boolean {
    return type === 'agent_delegate_started'
        || type === 'agent_handoff_accepted'
        || type === 'task_return_completed'
        || type.startsWith('agent_invocation_')
        || type.startsWith('agent_task_');
}

function normalizeProjectionInvocation(value: unknown, index: number): TimelineProjectionInvocation {
    if (!plainObject(value)) {
        throw new Error(`agent.timeline_projection_invalid: invocations[${index}] must be an object`);
    }
    return {
        invocationId: requiredString(value.invocationId, `invocations[${index}].invocationId`),
        parentInvocationId: optionalString(value.parentInvocationId),
        profileId: requiredString(value.profileId, `invocations[${index}].profileId`),
        kind: requiredString(value.kind, `invocations[${index}].kind`),
        status: requiredString(value.status, `invocations[${index}].status`),
        exitPolicy: requiredString(value.exitPolicy, `invocations[${index}].exitPolicy`),
        createdAt: requiredString(value.createdAt, `invocations[${index}].createdAt`),
        updatedAt: requiredString(value.updatedAt, `invocations[${index}].updatedAt`),
    };
}

function normalizeProjectionDelegationEdge(value: unknown, index: number): TimelineDelegationEdge {
    if (!plainObject(value)) {
        throw new Error(`agent.timeline_projection_invalid: delegationEdges[${index}] must be an object`);
    }
    const targetInvocationId = requiredString(value.targetInvocationId, `delegationEdges[${index}].targetInvocationId`);
    return {
        taskId: requiredString(value.taskId, `delegationEdges[${index}].taskId`),
        sourceInvocationId: requiredString(value.sourceInvocationId, `delegationEdges[${index}].sourceInvocationId`),
        targetInvocationId,
        targetProfileId: requiredString(value.targetProfileId, `delegationEdges[${index}].targetProfileId`),
        workspaceKey: requiredString(value.workspaceKey, `delegationEdges[${index}].workspaceKey`),
        continuation: requiredString(value.continuation, `delegationEdges[${index}].continuation`),
        status: requiredString(value.status, `delegationEdges[${index}].status`),
        resultRef: optionalString(value.resultRef),
        error: optionalString(value.error),
        createdAt: requiredString(value.createdAt, `delegationEdges[${index}].createdAt`),
        updatedAt: requiredString(value.updatedAt, `delegationEdges[${index}].updatedAt`),
    };
}

function requiredString(value: unknown, field: string): string {
    const normalized = typeof value === 'string' ? value.trim() : '';
    if (!normalized) throw new Error(`agent.timeline_projection_invalid: ${field} is required`);
    return normalized;
}

function optionalString(value: unknown): string {
    return typeof value === 'string' ? value.trim() : '';
}

function plainObject(value: unknown): value is Record<string, unknown> {
    return Boolean(value) && typeof value === 'object' && !Array.isArray(value);
}
