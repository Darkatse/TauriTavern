// @ts-check

import { createChatProjection } from '../../kernel/chat-surface/projection.js';
import {
    assertActiveRuntimeSources,
    assertReleasedRuntimeSources,
    assertResidencyContents,
    closeMessageResidencies,
} from './message-residency.js';

/**
 * Compatibility ingress for legacy renderers which directly remove roots.
 * It validates the complete surviving projection before publishing the change.
 *
 * @param {{
 *   elements: HTMLElement[];
 *   guardEnabled: boolean;
 *   residencies: ReturnType<import('./message-residency-index.js').createMessageResidencyIndex>;
 *   projection: any;
 *   messageCount: number;
 *   trueTailId: number;
 *   domAdapter: ReturnType<import('../../adapters/chat-surface/chat-dom-adapter.js').createChatDomAdapter>;
 * }} input
 */
export function reconcileLegacyMessageRemovals({
    elements,
    guardEnabled,
    residencies,
    projection,
    messageCount,
    trueTailId,
    domAdapter,
}) {
    if (guardEnabled) {
        throw new Error('ChatSurface rejected an external message removal in strict mode');
    }
    const removed = [...new Set(elements.map(element => residencies.getByElement(element)).filter(Boolean))];
    if (removed.length === 0) {
        return null;
    }
    if (removed.some(record => record.element.isConnected)) {
        throw new Error('ChatSurface legacy removal bridge rejected a connected message root');
    }
    if (removed.some(record => record.element.parentNode !== null)) {
        throw new Error('ChatSurface legacy removal bridge rejected a reparented message root');
    }

    const removedEntries = removed.map(record => ({ record }));
    closeMessageResidencies(removed, 'legacy-external-removal');
    assertReleasedRuntimeSources(removedEntries);
    assertResidencyContents(removedEntries, false);
    if (removed.some(record => record.element.isConnected)) {
        throw new Error('ChatSurface cleanup reconnected a legacy-removed message root');
    }
    if (removed.some(record => record.element.parentNode !== null)) {
        throw new Error('ChatSurface cleanup reparented a legacy-removed message root');
    }

    const removedMessageIds = new Set(removed.map(record => record.messageId));
    const nextProjection = createChatProjection(
        projection.indices.filter(/** @param {number} messageId */ messageId => !removedMessageIds.has(messageId)),
        { count: messageCount },
    );
    const retained = nextProjection.indices.map(messageId => {
        const record = residencies.getByMessageId(messageId);
        return { messageId, element: record.element, record };
    });
    domAdapter.assertCommitted(retained);
    assertResidencyContents(retained, true);
    assertActiveRuntimeSources(retained);

    residencies.remove(removed);
    domAdapter.syncLastMessage(trueTailId);
    return nextProjection;
}
