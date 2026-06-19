import { normalizeInvocationId } from './run-invocation-projector.js';

export function emptyTimelineProjection() {
    return {
        foregroundInvocationIds: [],
        invocations: [],
        delegationEdges: [],
    };
}

export function normalizeTimelineProjection(value) {
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
        invocations: value.invocations.map((invocation, index) => normalizeProjectionInvocation(invocation, index)),
        delegationEdges: value.delegationEdges.map((edge, index) => normalizeProjectionDelegationEdge(edge, index)),
    };
}

export function isTimelineProjectionStructuralEvent(type) {
    return type === 'agent_delegate_started'
        || type === 'agent_handoff_accepted'
        || type === 'task_return_completed'
        || String(type || '').startsWith('agent_invocation_')
        || String(type || '').startsWith('agent_task_');
}

function normalizeProjectionInvocation(invocation, index) {
    if (!plainObject(invocation)) {
        throw new Error(`agent.timeline_projection_invalid: invocations[${index}] must be an object`);
    }
    return {
        invocationId: requiredProjectionString(invocation.invocationId, `invocations[${index}].invocationId`),
        parentInvocationId: optionalProjectionString(invocation.parentInvocationId),
        profileId: requiredProjectionString(invocation.profileId, `invocations[${index}].profileId`),
        kind: requiredProjectionString(invocation.kind, `invocations[${index}].kind`),
        status: requiredProjectionString(invocation.status, `invocations[${index}].status`),
        exitPolicy: requiredProjectionString(invocation.exitPolicy, `invocations[${index}].exitPolicy`),
        createdAt: requiredProjectionString(invocation.createdAt, `invocations[${index}].createdAt`),
        updatedAt: requiredProjectionString(invocation.updatedAt, `invocations[${index}].updatedAt`),
    };
}

function normalizeProjectionDelegationEdge(edge, index) {
    if (!plainObject(edge)) {
        throw new Error(`agent.timeline_projection_invalid: delegationEdges[${index}] must be an object`);
    }
    const targetInvocationId = requiredProjectionString(edge.targetInvocationId, `delegationEdges[${index}].targetInvocationId`);
    return {
        taskId: requiredProjectionString(edge.taskId, `delegationEdges[${index}].taskId`),
        sourceInvocationId: requiredProjectionString(edge.sourceInvocationId, `delegationEdges[${index}].sourceInvocationId`),
        targetInvocationId,
        childInvocationId: targetInvocationId,
        targetProfileId: requiredProjectionString(edge.targetProfileId, `delegationEdges[${index}].targetProfileId`),
        workspaceKey: requiredProjectionString(edge.workspaceKey, `delegationEdges[${index}].workspaceKey`),
        continuation: requiredProjectionString(edge.continuation, `delegationEdges[${index}].continuation`),
        status: requiredProjectionString(edge.status, `delegationEdges[${index}].status`),
        resultRef: optionalProjectionString(edge.resultRef),
        error: optionalProjectionString(edge.error),
        createdAt: requiredProjectionString(edge.createdAt, `delegationEdges[${index}].createdAt`),
        updatedAt: requiredProjectionString(edge.updatedAt, `delegationEdges[${index}].updatedAt`),
    };
}

function requiredProjectionString(value, field) {
    const normalized = String(value || '').trim();
    if (!normalized) {
        throw new Error(`agent.timeline_projection_invalid: ${field} is required`);
    }
    return normalized;
}

function optionalProjectionString(value) {
    const normalized = String(value || '').trim();
    return normalized || '';
}

function plainObject(value) {
    return Boolean(value) && typeof value === 'object' && !Array.isArray(value);
}
