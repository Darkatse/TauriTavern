// @ts-check

import { resolveStableChatId } from './agent-chat-identity.js';

const DEFAULT_EVENT_POLL_MS = 500;
const MAX_RUN_LIST_LIMIT = 200;
const AGENT_RUN_STATUSES = new Set([
    'created',
    'initializing_workspace',
    'assembling_context',
    'calling_model',
    'dispatching_tool',
    'applying_workspace_patch',
    'creating_checkpoint',
    'awaiting_host_commit',
    'finishing',
    'completed',
    'partial_success',
    'cancelling',
    'cancelled',
    'failed',
]);

/**
 * @param {{ safeInvoke: (command: string, args?: any) => Promise<any> }} deps
 */
export function createAgentRunRuntimeApi({ safeInvoke }) {
    async function cancel(runId) {
        const normalizedRunId = requireRunId(runId);
        return safeInvoke('cancel_agent_run', { dto: { runId: normalizedRunId } });
    }

    async function listRuns(input = {}) {
        if (!isPlainObject(input)) {
            throw new Error('Agent listRuns input must be an object');
        }

        const chatRef = input.chatRef;
        if (chatRef != null && !isPlainObject(chatRef)) {
            throw new Error('chatRef must be an object');
        }

        const stableChatId = normalizeOptionalString(input.stableChatId ?? input.stable_chat_id);
        const statuses = normalizeRunStatuses(input.statuses);
        const before = normalizeRunListBefore(input.before);
        const limit = normalizeRunListLimit(input.limit);

        return safeInvoke('list_agent_runs', {
            dto: {
                ...(chatRef ? { chatRef } : {}),
                ...(stableChatId ? { stableChatId } : {}),
                ...(statuses ? { statuses } : {}),
                ...(before ? { before } : {}),
                ...(limit == null ? {} : { limit }),
            },
        });
    }

    async function readEvents(input) {
        const runId = requireRunId(input?.runId);
        const hasInvocationId = Object.prototype.hasOwnProperty.call(input || {}, 'invocationId');
        const invocationId = String(input?.invocationId || '').trim();
        if (hasInvocationId && !invocationId) {
            throw new Error('invocationId cannot be empty');
        }
        return safeInvoke('read_agent_run_events', {
            dto: {
                runId,
                afterSeq: input?.afterSeq,
                beforeSeq: input?.beforeSeq,
                limit: input?.limit,
                ...(invocationId ? { invocationId } : {}),
                ...(input?.includeTimelineProjection === true ? { includeTimelineProjection: true } : {}),
            },
        });
    }

    async function readWorkspaceFile(input) {
        const runId = requireRunId(input?.runId);
        const path = String(input?.path || '').trim();
        if (!path) {
            throw new Error('path is required');
        }

        return safeInvoke('read_agent_workspace_file', { dto: { runId, path } });
    }

    async function readModelTurn(input) {
        const runId = requireRunId(input?.runId);
        const round = Number(input?.round);
        if (!Number.isInteger(round) || round <= 0) {
            throw new Error('round must be a positive integer');
        }
        const maxChars = input?.maxChars == null ? undefined : Number(input.maxChars);
        if (maxChars != null && (!Number.isInteger(maxChars) || maxChars <= 0)) {
            throw new Error('maxChars must be a positive integer');
        }
        const invocationId = String(input?.invocationId || '').trim();

        return safeInvoke('read_agent_model_turn', {
            dto: {
                runId,
                round,
                ...(invocationId ? { invocationId } : {}),
                ...(maxChars == null ? {} : { maxChars }),
            },
        });
    }

    async function pruneChatPersistentStates(input = {}) {
        if (!input || typeof input !== 'object' || Array.isArray(input)) {
            throw new Error('Agent pruneChatPersistentStates input must be an object');
        }
        const chatRef = input.chatRef || window.__TAURITAVERN__?.api?.chat?.current?.ref?.();
        if (!chatRef || typeof chatRef !== 'object') {
            throw new Error('chatRef is required');
        }
        const stableChatId = String(input.stableChatId || '').trim() || await resolveStableChatId(chatRef);
        if (!stableChatId) {
            throw new Error('stableChatId is required');
        }
        const candidateStateIdsInput = Object.prototype.hasOwnProperty.call(input, 'candidateStateIds')
            ? input.candidateStateIds
            : input.candidate_state_ids;
        const candidateStateIds = normalizeStateIdList(candidateStateIdsInput);

        return safeInvoke('prune_agent_chat_persistent_states', {
            dto: {
                chatRef,
                stableChatId,
                candidateStateIds,
            },
        });
    }

    function subscribe(runId, handler, options = {}) {
        const normalizedRunId = requireRunId(runId);
        if (typeof handler !== 'function') {
            throw new Error('handler is required');
        }

        const intervalMs = normalizePollInterval(options?.intervalMs);
        let afterSeq = Number(options?.afterSeq || 0);
        let stopped = false;
        let timer = null;

        const tick = async () => {
            if (stopped) {
                return;
            }

            try {
                const result = await readEvents({
                    runId: normalizedRunId,
                    afterSeq,
                    limit: options?.limit || 100,
                });
                const events = Array.isArray(result?.events) ? result.events : [];
                for (const event of events) {
                    afterSeq = Math.max(afterSeq, Number(event?.seq || 0));
                    handler(event);
                }
            } catch (error) {
                if (typeof options?.onError === 'function') {
                    options.onError(error);
                } else {
                    queueMicrotask(() => {
                        throw error;
                    });
                }
            } finally {
                if (!stopped) {
                    timer = setTimeout(tick, intervalMs);
                }
            }
        };

        timer = setTimeout(tick, 0);

        return function unsubscribe() {
            stopped = true;
            if (timer !== null) {
                clearTimeout(timer);
                timer = null;
            }
        };
    }

    return {
        cancel,
        listRuns,
        readEvents,
        readWorkspaceFile,
        readModelTurn,
        pruneChatPersistentStates,
        subscribe,
    };
}

function requireRunId(value) {
    const runId = String(value || '').trim();
    if (!runId) {
        throw new Error('runId is required');
    }
    return runId;
}

function normalizeOptionalString(value) {
    if (value == null || value === '') {
        return undefined;
    }
    const text = String(value).trim();
    return text || undefined;
}

function normalizeRunStatuses(value) {
    if (value == null) {
        return undefined;
    }
    if (!Array.isArray(value)) {
        throw new Error('statuses must be an array');
    }

    const statuses = [];
    const seen = new Set();
    for (const item of value) {
        const status = String(item ?? '').trim();
        if (!status) {
            throw new Error('statuses contains an empty status');
        }
        if (!AGENT_RUN_STATUSES.has(status)) {
            throw new Error(`unknown agent run status: ${status}`);
        }
        if (seen.has(status)) {
            continue;
        }
        seen.add(status);
        statuses.push(status);
    }
    return statuses.length ? statuses : undefined;
}

function normalizeRunListBefore(value) {
    if (value == null) {
        return undefined;
    }
    if (!isPlainObject(value)) {
        throw new Error('before must be an object');
    }

    const runId = String(value.runId ?? value.run_id ?? '').trim();
    if (!runId) {
        throw new Error('before.runId is required');
    }
    const createdAt = normalizeRunListCursorTimestamp(value.createdAt ?? value.created_at);
    return { createdAt, runId };
}

function normalizeRunListCursorTimestamp(value) {
    if (value instanceof Date) {
        if (Number.isNaN(value.getTime())) {
            throw new Error('before.createdAt must be a valid timestamp');
        }
        return value.toISOString();
    }
    const timestamp = String(value ?? '').trim();
    if (!timestamp) {
        throw new Error('before.createdAt is required');
    }
    const parsed = new Date(timestamp);
    if (Number.isNaN(parsed.getTime())) {
        throw new Error('before.createdAt must be a valid timestamp');
    }
    return parsed.toISOString();
}

function normalizeRunListLimit(value) {
    if (value == null) {
        return undefined;
    }
    const limit = Number(value);
    if (!Number.isInteger(limit) || limit <= 0 || limit > MAX_RUN_LIST_LIMIT) {
        throw new Error(`limit must be an integer between 1 and ${MAX_RUN_LIST_LIMIT}`);
    }
    return limit;
}

function normalizeStateIdList(value) {
    if (!Array.isArray(value)) {
        throw new Error('candidateStateIds must be an array');
    }

    const stateIds = [];
    const seen = new Set();
    for (const item of value) {
        const stateId = String(item ?? '').trim();
        if (!stateId) {
            throw new Error('candidateStateIds contains an empty state id');
        }
        if (seen.has(stateId)) {
            continue;
        }
        seen.add(stateId);
        stateIds.push(stateId);
    }
    return stateIds;
}

function normalizePollInterval(value) {
    const intervalMs = Number(value || DEFAULT_EVENT_POLL_MS);
    if (!Number.isFinite(intervalMs) || intervalMs < 100) {
        return DEFAULT_EVENT_POLL_MS;
    }
    return intervalMs;
}

function isPlainObject(value) {
    return Boolean(value) && typeof value === 'object' && !Array.isArray(value);
}
