import { event_types, eventSource } from '../../../events.js';
import { errorText, requireLlmConnectionsApi, requireSillyTavernContext } from './host-api.js';
import { translateAgentSystem as tr } from './i18n.js';
import {
    buildLlmConnectionFromModelTarget,
    findModelTargetForBinding,
    findModelTargetForConnectionRef,
    listSavedModelTargets as listSavedModelTargetsFromContext,
    modelBindingFromTarget,
    modelTargetConnectionRef,
    modelTargetIdFromConnectionRef,
    saveModelTargetAsLlmConnection as saveModelTargetAsLlmConnectionWithApi,
} from '../../../tauritavern/agent/model-target-llm-connection.js';

export {
    buildLlmConnectionFromModelTarget,
    findModelTargetForBinding,
    findModelTargetForConnectionRef,
    modelBindingFromTarget,
    modelTargetConnectionRef,
    modelTargetIdFromConnectionRef,
};

let stopModelTargetLlmConnectionSync = null;

export function listSavedModelTargets() {
    return listSavedModelTargetsFromContext(requireSillyTavernContext());
}

export async function saveModelTargetAsLlmConnection(target) {
    return saveModelTargetAsLlmConnectionWithApi(target, requireLlmConnectionsApi());
}

export async function syncModelTargetLlmConnection(target) {
    return saveModelTargetAsLlmConnection(target);
}

export async function syncSavedModelTargetLlmConnections() {
    const targets = listSavedModelTargets();
    const failed = [];

    for (const target of targets) {
        try {
            await syncModelTargetLlmConnection(target);
        } catch (error) {
            const invalidation = await invalidateModelTargetLlmConnection(target);
            failed.push({ target, error, invalidation });
            console.warn('[AgentSystem] Skipped Model Target LLM Connection sync', target, error, invalidation);
            if (invalidation.error) {
                reportModelTargetInvalidationFailure(target, invalidation);
            }
        }
    }

    return {
        synced: targets.length - failed.length,
        failed,
    };
}

export function startModelTargetLlmConnectionSync() {
    if (stopModelTargetLlmConnectionSync) {
        return stopModelTargetLlmConnectionSync;
    }

    const handleCreated = (target) => syncModelTargetLlmConnectionFromEvent(target);
    const handleUpdated = (_oldTarget, target) => syncModelTargetLlmConnectionFromEvent(target, { invalidateOnFailure: true });

    eventSource.on(event_types.MODEL_TARGET_CREATED, handleCreated);
    eventSource.on(event_types.MODEL_TARGET_UPDATED, handleUpdated);

    stopModelTargetLlmConnectionSync = () => {
        eventSource.removeListener(event_types.MODEL_TARGET_CREATED, handleCreated);
        eventSource.removeListener(event_types.MODEL_TARGET_UPDATED, handleUpdated);
        stopModelTargetLlmConnectionSync = null;
    };

    return stopModelTargetLlmConnectionSync;
}

export function subscribeModelTargetChanges(listener) {
    const handleCreated = (target) => listener({ type: 'created', target });
    const handleUpdated = (oldTarget, target) => listener({ type: 'updated', oldTarget, target });
    const handleDeleted = (target) => listener({ type: 'deleted', target });

    eventSource.on(event_types.MODEL_TARGET_CREATED, handleCreated);
    eventSource.on(event_types.MODEL_TARGET_UPDATED, handleUpdated);
    eventSource.on(event_types.MODEL_TARGET_DELETED, handleDeleted);

    return () => {
        eventSource.removeListener(event_types.MODEL_TARGET_CREATED, handleCreated);
        eventSource.removeListener(event_types.MODEL_TARGET_UPDATED, handleUpdated);
        eventSource.removeListener(event_types.MODEL_TARGET_DELETED, handleDeleted);
    };
}

async function syncModelTargetLlmConnectionFromEvent(target, options = {}) {
    try {
        await syncModelTargetLlmConnection(target);
    } catch (error) {
        const invalidation = options.invalidateOnFailure
            ? await invalidateModelTargetLlmConnection(target)
            : null;
        reportModelTargetSyncFailure(target, error, invalidation);
    }
}

async function invalidateModelTargetLlmConnection(target) {
    let connectionId = '';
    try {
        connectionId = modelTargetConnectionRef(target);
        await requireLlmConnectionsApi().delete({ connectionId });
        return { connectionId, deleted: true };
    } catch (error) {
        if (connectionId && isLlmConnectionNotFoundError(error)) {
            return { connectionId, deleted: false };
        }
        return { connectionId, deleted: false, error };
    }
}

function isLlmConnectionNotFoundError(error) {
    const message = errorText(error).toLowerCase();
    return message.includes('llm_connection.not_found') || message.includes('llm connection not found');
}

function reportModelTargetSyncFailure(target, error, invalidation = null) {
    const name = String(target?.name || target?.id || '').trim() || tr('savedModelTarget');
    const message = tr('modelTargetSyncFailed', { name, error: errorText(error) });
    console.error('[AgentSystem] Failed to sync Model Target as LLM Connection', target, error);
    window.toastr?.error?.(message);
    if (invalidation?.error) {
        reportModelTargetInvalidationFailure(target, invalidation);
    }
}

function reportModelTargetInvalidationFailure(target, invalidation) {
    const name = String(target?.name || target?.id || '').trim() || tr('savedModelTarget');
    const message = tr('modelTargetInvalidationFailed', {
        name,
        error: errorText(invalidation.error),
    });
    console.error('[AgentSystem] Failed to invalidate stale Model Target LLM Connection', target, invalidation.error);
    window.toastr?.error?.(message);
}
