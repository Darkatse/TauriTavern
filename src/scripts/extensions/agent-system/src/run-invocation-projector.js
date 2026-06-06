export const ROOT_INVOCATION_ID = 'inv_root';

const SUBAGENT_COLORS = Object.freeze([
    '#5fa6a0',
    '#7c9bd6',
    '#c59a50',
    '#bf7493',
    '#7daf63',
    '#b084cc',
]);

const TASK_TERMINAL_STATUSES = new Set(['completed', 'failed', 'cancelled']);
const RETURN_TO_PARENT_CONTINUATION = 'return_to_parent';
const TRANSFER_CONTROL_CONTINUATION = 'transfer_control';

export function projectAgentInvocations(events) {
    const invocations = new Map();
    const tasks = new Map();

    ensureInvocation(invocations, ROOT_INVOCATION_ID, {
        invocationId: ROOT_INVOCATION_ID,
        kind: 'root',
        status: 'running',
        profileId: '',
        parentInvocationId: '',
        firstSeq: 0,
        latestSeq: 0,
    });

    for (const event of Array.isArray(events) ? events : []) {
        const type = String(event?.type || '');
        const payload = plainObject(event?.payload) ? event.payload : {};
        const seq = Number(event?.seq || 0);

        if (type.startsWith('agent_invocation_')) {
            const invocationId = String(payload.invocationId || '').trim();
            if (invocationId) {
                ensureInvocation(invocations, invocationId, {
                    invocationId,
                    parentInvocationId: stringField(payload.parentInvocationId),
                    profileId: stringField(payload.profileId),
                    kind: stringField(payload.kind),
                    status: statusFromInvocationEvent(type, payload.status),
                    taskId: stringField(payload.taskId),
                    firstSeq: seq,
                    latestSeq: seq,
                });
            }
        }

        if (type === 'agent_task_registered'
            || type === 'agent_task_queued'
            || type === 'agent_task_started'
            || type === 'agent_task_completed'
            || type === 'agent_task_failed'
            || type === 'agent_task_cancelled'
            || type === 'agent_delegate_started') {
            const taskId = String(payload.taskId || '').trim();
            if (taskId) {
                ensureTask(tasks, taskId, {
                    taskId,
                    parentInvocationId: stringField(payload.parentInvocationId),
                    childInvocationId: stringField(payload.childInvocationId),
                    targetProfileId: stringField(payload.targetProfileId),
                    workspaceKey: stringField(payload.workspaceKey),
                    continuation: continuationFromTaskEvent(type, payload),
                    status: statusFromTaskEvent(type, payload.status),
                    resultRef: stringField(payload.resultRef),
                    summaryRef: stringField(payload.summaryRef),
                    error: stringField(payload.error),
                    firstSeq: seq,
                    latestSeq: seq,
                });
            }
        }

        if (type === 'agent_handoff_accepted') {
            const taskId = String(payload.taskId || '').trim();
            if (taskId) {
                ensureTask(tasks, taskId, {
                    taskId,
                    parentInvocationId: stringField(payload.sourceInvocationId),
                    childInvocationId: stringField(payload.newInvocationId),
                    targetProfileId: stringField(payload.targetProfileId),
                    workspaceKey: stringField(payload.workspaceKey),
                    continuation: TRANSFER_CONTROL_CONTINUATION,
                    status: 'queued',
                    firstSeq: seq,
                    latestSeq: seq,
                });
            }
        }

        if (type === 'task_return_completed') {
            const taskId = String(payload.taskId || '').trim();
            if (taskId) {
                ensureTask(tasks, taskId, {
                    taskId,
                    childInvocationId: stringField(payload.childInvocationId),
                    status: statusFromTaskEvent(type, payload.status),
                    resultRef: stringField(payload.resultRef),
                    summaryRef: stringField(payload.summaryRef),
                    latestSeq: seq,
                });
            }
        }
    }

    const taskList = [...tasks.values()];
    const subAgentTasks = taskList
        .filter((task) => task.continuation === RETURN_TO_PARENT_CONTINUATION)
        .filter((task) => task.childInvocationId)
        .map((task, index) => ({
            ...task,
            color: SUBAGENT_COLORS[index % SUBAGENT_COLORS.length],
            displayName: task.targetProfileId || task.workspaceKey || task.childInvocationId,
            invocation: invocations.get(task.childInvocationId) || null,
        }))
        .sort((a, b) => a.firstSeq - b.firstSeq || a.taskId.localeCompare(b.taskId));
    const handoffTasks = taskList
        .filter((task) => task.continuation === TRANSFER_CONTROL_CONTINUATION)
        .filter((task) => task.childInvocationId)
        .map((task) => ({
            ...task,
            displayName: task.targetProfileId || task.workspaceKey || task.childInvocationId,
            invocation: invocations.get(task.childInvocationId) || null,
        }))
        .sort((a, b) => a.firstSeq - b.firstSeq || a.taskId.localeCompare(b.taskId));
    const foregroundInvocationIds = foregroundChainInvocationIds(invocations, handoffTasks);

    return {
        invocations,
        tasks,
        subAgentTasks,
        handoffTasks,
        foregroundInvocationIds,
        runningSubAgentCount: subAgentTasks.filter((task) => isActiveTaskStatus(task.status)).length,
        terminalSubAgentCount: subAgentTasks.filter((task) => TASK_TERMINAL_STATUSES.has(task.status)).length,
        failedSubAgentCount: subAgentTasks.filter((task) => task.status === 'failed').length,
    };
}

export function eventsForInvocation(events, invocationId) {
    const normalized = normalizeInvocationId(invocationId);
    return (Array.isArray(events) ? events : []).filter((event) => eventBelongsToInvocation(event, normalized));
}

export function eventBelongsToInvocation(event, invocationId) {
    const normalized = normalizeInvocationId(invocationId);
    const payload = plainObject(event?.payload) ? event.payload : {};
    const type = String(event?.type || '');

    if (normalized === ROOT_INVOCATION_ID) {
        if (type.startsWith('run_')) {
            return true;
        }
        if (type === 'agent_delegate_started') {
            return normalizeInvocationId(payload.parentInvocationId) === ROOT_INVOCATION_ID;
        }
        if (type.startsWith('agent_task_')) {
            return false;
        }
        if (payload.childInvocationId && normalizeInvocationId(payload.childInvocationId) !== ROOT_INVOCATION_ID) {
            return false;
        }
        return normalizeInvocationId(payload.invocationId) === ROOT_INVOCATION_ID;
    }

    return normalizeInvocationId(payload.invocationId) === normalized
        || normalizeInvocationId(payload.childInvocationId) === normalized
        || normalizeInvocationId(payload.scope?.invocationId) === normalized;
}

export function normalizeInvocationId(value) {
    return String(value || '').trim() || ROOT_INVOCATION_ID;
}

export function isRootInvocation(value) {
    return normalizeInvocationId(value) === ROOT_INVOCATION_ID;
}

export function isActiveTaskStatus(status) {
    return status === 'queued' || status === 'running';
}

function ensureInvocation(invocations, invocationId, patch) {
    const current = invocations.get(invocationId) || {
        invocationId,
        parentInvocationId: '',
        profileId: '',
        kind: '',
        status: '',
        taskId: '',
        firstSeq: Number.POSITIVE_INFINITY,
        latestSeq: 0,
    };
    const next = mergeRecord(current, patch);
    invocations.set(invocationId, next);
    return next;
}

function ensureTask(tasks, taskId, patch) {
    const current = tasks.get(taskId) || {
        taskId,
        parentInvocationId: '',
        childInvocationId: '',
        targetProfileId: '',
        workspaceKey: '',
        continuation: '',
        status: 'queued',
        resultRef: '',
        summaryRef: '',
        error: '',
        firstSeq: Number.POSITIVE_INFINITY,
        latestSeq: 0,
    };
    const next = mergeRecord(current, patch);
    tasks.set(taskId, next);
    return next;
}

function mergeRecord(current, patch) {
    const next = { ...current };
    for (const [key, value] of Object.entries(patch)) {
        if (value === '' || value == null) {
            continue;
        }
        if (key === 'firstSeq') {
            next.firstSeq = Math.min(Number(next.firstSeq || Number.POSITIVE_INFINITY), Number(value));
            continue;
        }
        if (key === 'latestSeq') {
            next.latestSeq = Math.max(Number(next.latestSeq || 0), Number(value));
            continue;
        }
        next[key] = value;
    }
    if (!Number.isFinite(next.firstSeq)) {
        next.firstSeq = next.latestSeq || 0;
    }
    return next;
}

function statusFromInvocationEvent(type, status) {
    const explicit = stringField(status);
    if (explicit) {
        return explicit;
    }
    if (type.endsWith('_started')) {
        return 'running';
    }
    if (type.endsWith('_completed')) {
        return 'completed';
    }
    if (type.endsWith('_failed')) {
        return 'failed';
    }
    if (type.endsWith('_cancelled')) {
        return 'cancelled';
    }
    return '';
}

function statusFromTaskEvent(type, status) {
    const explicit = stringField(status);
    if (explicit) {
        return explicit;
    }
    if (type === 'agent_delegate_started' || type.endsWith('_started')) {
        return 'running';
    }
    if (type.endsWith('_completed') || type === 'task_return_completed') {
        return 'completed';
    }
    if (type.endsWith('_failed')) {
        return 'failed';
    }
    if (type.endsWith('_cancelled')) {
        return 'cancelled';
    }
    return 'queued';
}

function continuationFromTaskEvent(type, payload) {
    const explicit = normalizeContinuation(payload.continuation);
    if (explicit) {
        return explicit;
    }
    if (type === 'agent_delegate_started') {
        return RETURN_TO_PARENT_CONTINUATION;
    }
    return '';
}

function normalizeContinuation(value) {
    const normalized = stringField(value);
    if (normalized === 'returnToParent' || normalized === RETURN_TO_PARENT_CONTINUATION) {
        return RETURN_TO_PARENT_CONTINUATION;
    }
    if (normalized === 'transferControl' || normalized === TRANSFER_CONTROL_CONTINUATION) {
        return TRANSFER_CONTROL_CONTINUATION;
    }
    return normalized;
}

function foregroundChainInvocationIds(invocations, handoffTasks) {
    const entries = new Map();
    entries.set(ROOT_INVOCATION_ID, 0);

    for (const task of handoffTasks) {
        const invocationId = normalizeInvocationId(task.childInvocationId);
        if (!isRootInvocation(invocationId)) {
            entries.set(invocationId, earliestSeq(entries.get(invocationId), task.firstSeq));
        }
    }

    for (const invocation of invocations.values()) {
        const invocationId = normalizeInvocationId(invocation.invocationId);
        if (invocation.kind === 'handoff' && !isRootInvocation(invocationId)) {
            entries.set(invocationId, earliestSeq(entries.get(invocationId), invocation.firstSeq));
        }
    }

    return [...entries.entries()]
        .sort((left, right) => left[1] - right[1] || left[0].localeCompare(right[0]))
        .map(([invocationId]) => invocationId);
}

function earliestSeq(current, candidate) {
    const normalized = Number(candidate);
    if (!Number.isFinite(normalized)) {
        return current ?? Number.MAX_SAFE_INTEGER;
    }
    return Math.min(current ?? normalized, normalized);
}

function stringField(value) {
    return String(value || '').trim();
}

function plainObject(value) {
    return Boolean(value) && typeof value === 'object' && !Array.isArray(value);
}
