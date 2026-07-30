// @ts-check

import { createChatProjection } from '../../kernel/chat-surface/projection.js';
import { createStructureSnapshotFactory } from '../../kernel/chat-surface/structure-snapshot.js';
import {
    assertActiveRuntimeSources,
    assertCommittedResidencies,
    assertResidencyContents,
    closeMessageResidencies,
    createMessageResidency,
    requireMessageContent,
} from './message-residency.js';
import { createMessageContentReconciler } from './message-content-reconciler.js';
import { createMessageResidencyIndex } from './message-residency-index.js';
import { reconcileLegacyMessageRemovals } from './legacy-removal-bridge.js';
import { createProjectionEntrypoints } from './projection-entrypoints.js';
import {
    createChatSurfaceParticipantLifecycle,
    createDetachedParticipantContext,
} from './participant-lifecycle.js';
import { createRuntimeAdmission } from './runtime-admission.js';
import { createChatSurfaceFaultAuthority } from './fault-authority.js';

/** @typedef {ReturnType<import('../../adapters/chat-surface/chat-dom-adapter.js').createChatDomAdapter>} ChatDomAdapter */

/** @param {string} message @param {unknown} cause */
function errorWithCause(message, cause) {
    const error = /** @type {Error & { cause?: unknown }} */ (new Error(message));
    error.cause = cause;
    return error;
}

/** @param {any} result */
function normalizeMaterializedElement(result) {
    const element = result instanceof HTMLElement ? result : result?.[0];
    if (!(element instanceof HTMLElement) || !element.matches('.mes')) {
        throw new Error('ChatSurface materializer must return a .mes HTMLElement');
    }
    if (element.isConnected || element.parentNode !== null) {
        throw new Error('ChatSurface materializer must return a parentless .mes element');
    }
    return element;
}

/**
 * @param {{
 *   getMessages: () => any[];
 *   materializeMessage: (input: {
 *     message: any;
 *     messageId: number;
 *     frontendSourceHandoffEvent: string | null;
 *     materializeOptions: any;
 *   }) => any;
 *   domAdapter: ChatDomAdapter;
 *   scrollAdapter: ReturnType<import('../../adapters/chat-surface/chat-scroll-adapter.js').createChatScrollAdapter>;
 *   participantRegistry: ReturnType<import('./participant-registry.js').createChatSurfaceParticipantRegistry>;
 * }} deps
 */
export function createChatSurfaceController({
    getMessages,
    materializeMessage,
    domAdapter,
    scrollAdapter,
    participantRegistry,
}) {
    if (typeof getMessages !== 'function' || typeof materializeMessage !== 'function') {
        throw new TypeError('ChatSurface requires getMessages and materializeMessage functions');
    }
    if (!domAdapter || !scrollAdapter || !participantRegistry) {
        throw new TypeError('ChatSurface requires DOM, scroll and participant adapters');
    }

    const structures = createStructureSnapshotFactory();
    let structure = structures.beginEpoch([]);
    let projection = createChatProjection([], { count: 0 });
    const residencies = createMessageResidencyIndex();
    const faults = createChatSurfaceFaultAuthority();
    const setFault = faults.set;
    /** @type {string | null} */
    let activeMutation = null;

    /** @type {readonly any[] | null} */
    let participantList = null;
    const participants = createChatSurfaceParticipantLifecycle({
        getParticipants: () => participantList ?? [],
        assertHealthy,
        root: domAdapter.root,
    });
    const runtimeAdmission = createRuntimeAdmission({
        activate: participants.activateCandidate,
        assertCandidate: participants.assertCandidate,
        runScheduled: operation => mutate('runtime-admission', operation),
        onFault: setFault,
    });

    function assertHealthy() {
        if (faults.current) {
            throw errorWithCause('ChatSurface epoch is faulted', faults.current);
        }
    }

    /** @template T @param {string} label @param {() => T} operation */
    function mutate(label, operation) {
        if (activeMutation) {
            throw setFault(new Error(`ChatSurface mutation ${label} reentered during ${activeMutation}`));
        }
        activeMutation = label;
        try {
            return operation();
        } finally {
            activeMutation = null;
        }
    }

    /** @template {(...args: any[]) => any} T @param {string} label @param {T} operation @returns {T} */
    function guardMutation(label, operation) {
        /** @param {...any} args */
        const guarded = (...args) => mutate(label, () => operation(...args));
        return /** @type {T} */ (guarded);
    }

    const contentReconciler = createMessageContentReconciler({
        getMessages,
        getStructure: () => structure,
        keyOf: structures.keyOf,
        root: domAdapter.root,
        residencies,
        participants,
        runtimeAdmission,
        discard: domAdapter.discard,
        assertHealthy,
        setFault,
    });

    /** @param {HTMLElement[]} elements */
    function reconcileExternalRemovals(elements) {
        if (faults.current) {
            return;
        }
        try {
            const nextProjection = reconcileLegacyMessageRemovals({
                elements,
                guardEnabled: domAdapter.isMutationGuardEnabled(),
                residencies,
                projection,
                messageCount: structure.keys.length,
                trueTailId: getMessages().length - 1,
                domAdapter,
            });
            if (!nextProjection) {
                return;
            }
            projection = nextProjection;
        } catch (error) {
            setFault(error);
        }
    }

    function reconcileDomResidency() {
        const detached = [...residencies.values()]
            .filter(record => record.element.parentElement !== domAdapter.root)
            .map(record => record.element);
        if (detached.length === 0) {
            return;
        }
        if (domAdapter.isMutationGuardEnabled()) {
            throw setFault(new Error('ChatSurface detected a missing committed message in strict mode'));
        }
        reconcileExternalRemovals(detached);
        assertHealthy();
    }

    /** @param {any[]} added @param {any[]} prepared @param {unknown} primaryError @param {string} reason */
    function discardAdded(added, prepared, primaryError, reason) {
        try {
            closeMessageResidencies([...added].reverse(), reason);
        } catch {
            // Preserve the transaction error which caused the discard.
        }
        domAdapter.discard([
            ...prepared.flatMap(item => item.candidates.map(
                /** @param {any} candidate */ candidate => candidate.source,
            )),
            ...added.flatMap(record => [record.contentElement, record.element]),
        ]);
        throw setFault(primaryError);
    }

    /**
     * @param {{
     *   nextStructure: any;
     *   indices: number[];
     *   messages: any[];
     *   replaceMessageIds?: number[];
     *   frontendSourceHandoffEvent?: string | null;
     *   materializeOptionsByMessageId?: Map<number, any>;
     * }} input
     */
    function commitProjection({
        nextStructure,
        indices,
        messages,
        replaceMessageIds = [],
        frontendSourceHandoffEvent = null,
        materializeOptionsByMessageId = new Map(),
    }) {
        const nextProjection = createChatProjection(indices, { count: messages.length });
        const replacements = new Set(replaceMessageIds);
        const sameProjection = replacements.size === 0
            && residencies.size === nextProjection.indices.length
            && nextProjection.indices.every((messageId, index) => {
                const key = nextStructure.keys[messageId];
                return projection.indices[index] === messageId && key && residencies.getByMountKey(key)?.messageId === messageId;
            });

        if (sameProjection) {
            try {
                assertCommittedResidencies(residencies, nextProjection, domAdapter);
            } catch (error) {
                throw setFault(error);
            }
            structure = nextStructure;
            for (const messageId of nextProjection.indices) {
                residencies.getByMessageId(messageId).message = messages[messageId];
            }
            domAdapter.syncLastMessage(messages.length - 1);
            assertHealthy();
            return;
        }

        const retained = new Set();
        /** @type {any[]} */
        const desired = [];
        /** @type {any[]} */
        const added = [];
        /** @type {any[]} */
        const prepared = [];
        try {
            if (!participantList && nextProjection.indices.length > 0) {
                participantList = participantRegistry.freeze(setFault);
            }
            for (const messageId of nextProjection.indices) {
                const mountKey = nextStructure.keys[messageId];
                const message = messages[messageId];
                if (!mountKey || !message) {
                    throw new Error(`ChatSurface projection cannot resolve message ${messageId}`);
                }
                const existing = residencies.getByMountKey(mountKey);
                const replace = Boolean(existing && existing.messageId !== messageId)
                    || replacements.has(messageId);
                if (existing && !replace) {
                    retained.add(existing);
                    desired.push({ record: existing, messageId, message });
                    continue;
                }

                const element = normalizeMaterializedElement(materializeMessage({
                    message,
                    messageId,
                    frontendSourceHandoffEvent,
                    materializeOptions: materializeOptionsByMessageId.get(messageId),
                }));
                const content = requireMessageContent(element);
                const record = createMessageResidency({
                    mountKey,
                    messageId,
                    message,
                    element,
                    content,
                });
                const item = {
                    record,
                    context: createDetachedParticipantContext(record, content),
                    contentRoot: element,
                    contentParent: content.parentNode,
                    candidates: [],
                    sourceOwners: new Map(),
                };
                desired.push({ record, messageId, message });
                added.push(record);
                prepared.push(item);
            }
            participants.prepare(prepared);
        } catch (error) {
            discardAdded(added, prepared, error, 'mount-preparation-failed');
        }

        const removed = [...residencies.values()].filter(record => !retained.has(record));
        const domChange = {
            removed: removed.map(record => ({ messageId: record.messageId, element: record.element })),
            desired: desired.map(entry => ({ messageId: entry.messageId, element: entry.record.element })),
        };
        try {
            domAdapter.validateCommit(domChange);
            closeMessageResidencies(removed, 'projection-unmount');
            assertHealthy();
            participants.assertPrepared(prepared);
            assertResidencyContents(desired, false);
            assertActiveRuntimeSources(desired);
            domAdapter.commit(domChange);
            for (const item of prepared) {
                item.record.runtimeSources = Object.freeze(item.candidates.map(
                    /** @param {any} candidate */ candidate => candidate.source,
                ));
            }
        } catch (error) {
            discardAdded(added, prepared, error, 'projection-commit-failed');
        }

        for (const entry of desired) {
            entry.record.messageId = entry.messageId;
            entry.record.message = entry.message;
        }
        residencies.replace(desired.map(entry => entry.record));

        structure = nextStructure;
        projection = nextProjection;
        domAdapter.syncLastMessage(messages.length - 1);
        try {
            assertCommittedResidencies(residencies, projection, domAdapter);
            participants.connected(prepared, ['didMount', 'didCommitContent']);
            runtimeAdmission.register(participants.runtimeCandidates(prepared));
            assertCommittedResidencies(residencies, projection, domAdapter);
            assertHealthy();
        } catch (error) {
            throw setFault(error);
        }
        assertHealthy();
    }

    const projectionEntrypoints = createProjectionEntrypoints({
        getMessages,
        getStructure: () => structure,
        structures,
        residencies,
        assertHealthy,
        reconcileDomResidency,
        commitProjection,
    });

    /** @param {number} messageId */
    function remountMessage(messageId) {
        if (!Number.isInteger(messageId)) {
            throw new TypeError('ChatSurface remountMessage requires an integer mesid');
        }
        if (!residencies.getByMessageId(messageId)) {
            return null;
        }
        return projectionEntrypoints.project({
            indices: projection.indices.slice(),
            replaceMessageIds: [messageId],
        });
    }

    /** @param {{ includeAuxiliary?: boolean }} [options] */
    function resetEpoch({ includeAuxiliary = false } = {}) {
        try {
            closeMessageResidencies([...residencies.values()].reverse(), 'epoch-reset');
            if (participantList) {
                participantRegistry.freeze(setFault);
            }
        } catch (error) {
            throw setFault(error);
        }
        try {
            const owned = [...residencies.values()];
            domAdapter.discard(owned.flatMap(record => [
                ...record.runtimeSources,
                record.contentElement,
                record.element,
            ]));
            includeAuxiliary ? domAdapter.clearAll() : domAdapter.clearMessages();
        } catch (error) {
            throw setFault(error);
        }
        residencies.clear();
        runtimeAdmission.resetEpoch();
        structure = structures.beginEpoch([]);
        projection = createChatProjection([], { count: 0 });
        faults.clear();
    }

    function snapshot() {
        return Object.freeze({
            messageCount: structure.keys.length,
            projection,
            mounted: Object.freeze([...residencies.values()].map(record => Object.freeze({
                mountKey: record.mountKey,
                messageId: record.messageId,
            }))),
            runtime: runtimeAdmission.snapshot(),
            fault: faults.current,
        });
    }

    /** @param {{ mode: 'eager' | 'managed'; maxActive?: number }} options */
    function configureRuntimeAdmission({ mode, maxActive }) {
        assertHealthy();
        runtimeAdmission.configure(mode, maxActive === undefined ? {} : { maxActive });
    }

    /** @param {{ messageIds: number[]; suspended?: boolean }} input */
    function setRuntimeDemand({ messageIds, suspended = false }) {
        assertHealthy();
        if (!Array.isArray(messageIds) || messageIds.some(messageId => !Number.isInteger(messageId))) {
            throw new TypeError('ChatSurface runtime demand requires integer messageIds');
        }
        if (new Set(messageIds).size !== messageIds.length) {
            throw new Error('ChatSurface runtime demand contains duplicate messageIds');
        }
        const mountKeys = messageIds.map(messageId => {
            const mountKey = structure.keys[messageId];
            if (!mountKey) {
                throw new Error(`ChatSurface runtime demand cannot resolve message ${messageId}`);
            }
            return mountKey;
        });
        runtimeAdmission.setDemand(mountKeys, { suspended });
    }

    function dispose() {
        try {
            resetEpoch();
        } finally {
            runtimeAdmission.dispose();
            domAdapter.dispose();
        }
    }
    return Object.freeze({
        reconcile: guardMutation('reconcile', projectionEntrypoints.reconcile),
        reconcilePlanned: guardMutation('reconcilePlanned', projectionEntrypoints.reconcilePlanned),
        project: guardMutation('project', projectionEntrypoints.project),
        reconcileMounted: guardMutation('reconcileMounted', projectionEntrypoints.reconcileMounted),
        reconcileExternalRemovals: guardMutation('reconcileExternalRemovals', reconcileExternalRemovals),
        remountMessage: guardMutation('remountMessage', remountMessage),
        updateContent: guardMutation('updateContent', contentReconciler.update),
        resetEpoch: guardMutation('resetEpoch', resetEpoch),
        configureRuntimeAdmission: guardMutation('configureRuntimeAdmission', configureRuntimeAdmission),
        setRuntimeDemand: guardMutation('setRuntimeDemand', setRuntimeDemand),
        getMessageElement: /** @param {number} messageId */ messageId => residencies.getByMessageId(messageId)?.element ?? null,
        ownsMessageElement: /** @param {HTMLElement} element */ element => residencies.getByElement(element)?.element === element,
        getMountedMessageIds: () => [...residencies.messageIds()],
        getFault: () => faults.current,
        setMutationGuardEnabled: domAdapter.setMutationGuardEnabled,
        snapshot,
        scroll: scrollAdapter,
        dispose: guardMutation('dispose', dispose),
        fault: setFault,
        subscribeFault: faults.subscribe,
    });
}
