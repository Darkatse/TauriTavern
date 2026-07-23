// @ts-check

/**
 * Separates structure-changing entrypoints from the controller's residency
 * transaction. Every path converges on one commitProjection callback.
 *
 * @param {{
 *   getMessages: () => any[];
 *   getStructure: () => any;
 *   structures: ReturnType<import('../../kernel/chat-surface/structure-snapshot.js').createStructureSnapshotFactory>;
 *   residencies: ReturnType<import('./message-residency-index.js').createMessageResidencyIndex>;
 *   assertHealthy: () => void;
 *   reconcileDomResidency: () => void;
 *   commitProjection: (input: any) => any;
 * }} deps
 */
export function createProjectionEntrypoints({
    getMessages,
    getStructure,
    structures,
    residencies,
    assertHealthy,
    reconcileDomResidency,
    commitProjection,
}) {
    /** @param {{ indices: number[]; messages?: any[]; replaceMessageIds?: number[]; frontendSourceHandoffEvent?: string | null; materializeOptionsByMessageId?: Map<number, any> }} input */
    function reconcile({ indices, messages = getMessages(), ...options }) {
        assertHealthy();
        reconcileDomResidency();
        if (!Array.isArray(messages)) {
            throw new TypeError('ChatSurface reconcile messages must be an array');
        }
        return commitProjection({ ...options, indices, messages, nextStructure: structures.update(messages) });
    }

    /** @param {{ messages?: any[]; plan: (structure: any) => { indices: number[] }; replaceMessageIds?: number[]; frontendSourceHandoffEvent?: string | null; materializeOptionsByMessageId?: Map<number, any> }} input */
    function reconcilePlanned({ messages = getMessages(), plan, ...options }) {
        assertHealthy();
        reconcileDomResidency();
        if (!Array.isArray(messages) || typeof plan !== 'function') {
            throw new TypeError('ChatSurface planned reconcile requires messages and a plan function');
        }
        const nextStructure = structures.update(messages);
        const intent = plan(nextStructure);
        if (!intent || !Array.isArray(intent.indices)) {
            throw new Error('ChatSurface projection plan must return indices');
        }
        return commitProjection({ ...options, indices: intent.indices, messages, nextStructure });
    }

    /** @param {{ indices: number[]; replaceMessageIds?: number[]; frontendSourceHandoffEvent?: string | null; materializeOptionsByMessageId?: Map<number, any> }} input */
    function project({ indices, ...options }) {
        assertHealthy();
        reconcileDomResidency();
        const structure = getStructure();
        const messages = getMessages();
        if (!Array.isArray(messages) || messages.length !== structure.keys.length) {
            throw new Error('ChatSurface structure must be reconciled before projection');
        }
        for (const messageId of indices) {
            if (structures.keyOf(messages[messageId]) !== structure.keys[messageId]) {
                throw new Error(`ChatSurface structure changed at projected message ${messageId}`);
            }
        }
        return commitProjection({ ...options, indices, messages, nextStructure: structure });
    }

    /** @param {{ messages?: any[]; includeMessageIds?: number[]; materializeOptionsByMessageId?: Map<number, any> }} [options] */
    function reconcileMounted({ messages = getMessages(), includeMessageIds = [], materializeOptionsByMessageId = new Map() } = {}) {
        assertHealthy();
        reconcileDomResidency();
        const nextStructure = structures.update(messages);
        const indexByKey = new Map(nextStructure.keys.map((key, index) => [key, index]));
        const indices = new Set(includeMessageIds);
        for (const mountKey of residencies.mountKeys()) {
            const index = indexByKey.get(mountKey);
            if (index !== undefined) {
                indices.add(index);
            }
        }
        return commitProjection({
            nextStructure,
            indices: [...indices].sort((left, right) => left - right),
            messages,
            materializeOptionsByMessageId,
        });
    }

    return Object.freeze({ reconcile, reconcilePlanned, project, reconcileMounted });
}
