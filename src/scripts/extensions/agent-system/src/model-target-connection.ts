import { errorText, requireLlmConnectionsApi, requireSillyTavernContext } from './host-api';
import { translateAgentSystem as tr } from './i18n';
import {
    findModelTargetForBinding,
    listSavedModelTargets as listSavedModelTargetsFromContext,
    modelBindingFromTarget,
    modelTargetConnectionRef,
    modelTargetIdFromConnectionRef,
    saveModelTargetAsLlmConnection as saveModelTargetAsLlmConnectionWithApi,
} from '../../../tauritavern/agent/model-target-llm-connection.js';

export type AgentModelTarget = ReturnType<typeof listSavedModelTargetsFromContext>[number];

export type AgentModelTargetChange =
    | { type: 'created'; target: AgentModelTarget }
    | { type: 'updated'; oldTarget: AgentModelTarget; target: AgentModelTarget }
    | { type: 'deleted'; target: AgentModelTarget };

type ModelTargetEventName = 'MODEL_TARGET_CREATED' | 'MODEL_TARGET_UPDATED' | 'MODEL_TARGET_DELETED';
type ModelTargetEventListener = (...targets: AgentModelTarget[]) => unknown;
type ModelTargetEventSource = {
    on: (eventName: string, listener: ModelTargetEventListener) => void;
    removeListener: (eventName: string, listener: ModelTargetEventListener) => void;
};
type ModelTargetEventTypes = Record<ModelTargetEventName, string>;
type ModelTargetContext = {
    eventSource?: ModelTargetEventSource;
    eventTypes?: Partial<ModelTargetEventTypes>;
};
type ModelTargetInvalidation = {
    connectionId: string;
    deleted: boolean;
    error?: unknown;
};
export { findModelTargetForBinding, modelBindingFromTarget, modelTargetIdFromConnectionRef };

let stopModelTargetLlmConnectionSync: (() => void) | null = null;

function requireModelTargetEvents(): { eventSource: ModelTargetEventSource; eventTypes: ModelTargetEventTypes } {
    const { eventSource, eventTypes } = requireSillyTavernContext() as ModelTargetContext;
    const created = eventTypes?.MODEL_TARGET_CREATED;
    const updated = eventTypes?.MODEL_TARGET_UPDATED;
    const deleted = eventTypes?.MODEL_TARGET_DELETED;

    if (
        typeof eventSource?.on !== 'function'
        || typeof eventSource?.removeListener !== 'function'
        || typeof created !== 'string' || created.length === 0
        || typeof updated !== 'string' || updated.length === 0
        || typeof deleted !== 'string' || deleted.length === 0
    ) {
        throw new Error('agent.model_target_events_unavailable: SillyTavern Model Target event contract is unavailable');
    }

    return {
        eventSource,
        eventTypes: {
            MODEL_TARGET_CREATED: created,
            MODEL_TARGET_UPDATED: updated,
            MODEL_TARGET_DELETED: deleted,
        },
    };
}

export function listSavedModelTargets(): AgentModelTarget[] {
    return listSavedModelTargetsFromContext(requireSillyTavernContext());
}

export async function saveModelTargetAsLlmConnection(
    target: AgentModelTarget,
): Promise<TauriTavernLlmConnectionDefinition> {
    return saveModelTargetAsLlmConnectionWithApi(target, requireLlmConnectionsApi());
}

export async function syncSavedModelTargetLlmConnections(): Promise<void> {
    const targets = listSavedModelTargets();

    for (const target of targets) {
        try {
            await saveModelTargetAsLlmConnection(target);
        } catch (error) {
            const invalidation = await invalidateModelTargetLlmConnection(target);
            console.warn('[AgentSystem] Skipped Model Target LLM Connection sync', target, error, invalidation);
            if (invalidation.error) {
                reportModelTargetInvalidationFailure(target, invalidation);
            }
        }
    }
}

export function startModelTargetLlmConnectionSync(): () => void {
    if (stopModelTargetLlmConnectionSync) {
        return stopModelTargetLlmConnectionSync;
    }

    const { eventSource, eventTypes } = requireModelTargetEvents();
    const handleCreated = (target: AgentModelTarget) => syncModelTargetLlmConnectionFromEvent(target);
    const handleUpdated = (_oldTarget: AgentModelTarget, target: AgentModelTarget) => (
        syncModelTargetLlmConnectionFromEvent(target, { invalidateOnFailure: true })
    );

    eventSource.on(eventTypes.MODEL_TARGET_CREATED, handleCreated);
    eventSource.on(eventTypes.MODEL_TARGET_UPDATED, handleUpdated);

    stopModelTargetLlmConnectionSync = () => {
        eventSource.removeListener(eventTypes.MODEL_TARGET_CREATED, handleCreated);
        eventSource.removeListener(eventTypes.MODEL_TARGET_UPDATED, handleUpdated);
        stopModelTargetLlmConnectionSync = null;
    };

    return stopModelTargetLlmConnectionSync;
}

export function subscribeModelTargetChanges(
    listener: (change: AgentModelTargetChange) => void,
): () => void {
    const { eventSource, eventTypes } = requireModelTargetEvents();
    const handleCreated = (target: AgentModelTarget) => listener({ type: 'created', target });
    const handleUpdated = (oldTarget: AgentModelTarget, target: AgentModelTarget) => (
        listener({ type: 'updated', oldTarget, target })
    );
    const handleDeleted = (target: AgentModelTarget) => listener({ type: 'deleted', target });

    eventSource.on(eventTypes.MODEL_TARGET_CREATED, handleCreated);
    eventSource.on(eventTypes.MODEL_TARGET_UPDATED, handleUpdated);
    eventSource.on(eventTypes.MODEL_TARGET_DELETED, handleDeleted);

    return () => {
        eventSource.removeListener(eventTypes.MODEL_TARGET_CREATED, handleCreated);
        eventSource.removeListener(eventTypes.MODEL_TARGET_UPDATED, handleUpdated);
        eventSource.removeListener(eventTypes.MODEL_TARGET_DELETED, handleDeleted);
    };
}

async function syncModelTargetLlmConnectionFromEvent(
    target: AgentModelTarget,
    options: { invalidateOnFailure?: boolean } = {},
): Promise<void> {
    try {
        await saveModelTargetAsLlmConnection(target);
    } catch (error) {
        const invalidation = options.invalidateOnFailure
            ? await invalidateModelTargetLlmConnection(target)
            : null;
        reportModelTargetSyncFailure(target, error, invalidation);
    }
}

async function invalidateModelTargetLlmConnection(target: AgentModelTarget): Promise<ModelTargetInvalidation> {
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

function isLlmConnectionNotFoundError(error: unknown): boolean {
    const message = errorText(error).toLowerCase();
    return message.includes('llm_connection.not_found') || message.includes('llm connection not found');
}

function reportModelTargetSyncFailure(
    target: AgentModelTarget,
    error: unknown,
    invalidation: ModelTargetInvalidation | null = null,
): void {
    const name = (target.name || target.id).trim() || tr('savedModelTarget');
    const message = tr('modelTargetSyncFailed', { name, error: errorText(error) });
    console.error('[AgentSystem] Failed to sync Model Target as LLM Connection', target, error);
    window.toastr?.error?.(message);
    if (invalidation?.error) {
        reportModelTargetInvalidationFailure(target, invalidation);
    }
}

function reportModelTargetInvalidationFailure(
    target: AgentModelTarget,
    invalidation: ModelTargetInvalidation,
): void {
    const name = (target.name || target.id).trim() || tr('savedModelTarget');
    const message = tr('modelTargetInvalidationFailed', {
        name,
        error: errorText(invalidation.error),
    });
    console.error('[AgentSystem] Failed to invalidate stale Model Target LLM Connection', target, invalidation.error);
    window.toastr?.error?.(message);
}
