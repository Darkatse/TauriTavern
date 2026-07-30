// @ts-check

import {
    assertActiveRuntimeSources,
    createContentLease,
    requireMessageContent,
} from './message-residency.js';
import { createDetachedParticipantContext } from './participant-lifecycle.js';

/**
 * Owns the content-version transaction for one already-mounted message.
 * Structure and root identity are checked before the old content lease closes.
 *
 * @param {{
 *   getMessages: () => any[];
 *   getStructure: () => any;
 *   keyOf: (message: any) => string | null;
 *   root: HTMLElement;
 *   residencies: ReturnType<import('./message-residency-index.js').createMessageResidencyIndex>;
 *   participants: ReturnType<import('./participant-lifecycle.js').createChatSurfaceParticipantLifecycle>;
 *   runtimeAdmission: ReturnType<import('./runtime-admission.js').createRuntimeAdmission>;
 *   discard: (elements: Iterable<HTMLElement>) => void;
 *   assertHealthy: () => void;
 *   setFault: (error: unknown) => Error;
 * }} deps
 */
export function createMessageContentReconciler({
    getMessages,
    getStructure,
    keyOf,
    root,
    residencies,
    participants,
    runtimeAdmission,
    discard,
    assertHealthy,
    setFault,
}) {
    /** @param {any} record @param {HTMLElement} expectedContent */
    function assertLiveContent(record, expectedContent) {
        try {
            if (
                record.element.parentElement !== root
                || record.element.getAttribute('mesid') !== String(record.messageId)
                || requireMessageContent(record.element) !== expectedContent
                || expectedContent.parentElement !== record.contentParent
            ) {
                throw new Error(`ChatSurface message ${record.messageId} DOM identity changed during content reconciliation`);
            }
        } catch (error) {
            const failure = /** @type {Error & { cause?: unknown }} */ (
                new Error(`ChatSurface message ${record.messageId} DOM identity changed during content reconciliation`)
            );
            failure.cause = error;
            throw setFault(failure);
        }
        assertHealthy();
    }

    /** @param {HTMLElement} messageElement @param {{ content: HTMLElement; commit: () => unknown }} transaction @param {{ notifyParticipants?: boolean }} [options] */
    function update(messageElement, transaction, { notifyParticipants = true } = {}) {
        assertHealthy();
        if (!(messageElement instanceof HTMLElement) || !transaction || typeof transaction.commit !== 'function') {
            throw new TypeError('ChatSurface updateContent requires a message element and content transaction');
        }
        if (!(transaction.content instanceof HTMLElement) || transaction.content.isConnected) {
            throw new Error('ChatSurface content transaction must expose detached content');
        }
        const record = residencies.getByElement(messageElement);
        if (!record) {
            throw new Error('ChatSurface cannot update content for an uncommitted message element');
        }
        const messageId = record.messageId;
        if (messageElement.parentElement !== root || messageElement.getAttribute('mesid') !== String(messageId)) {
            throw setFault(new Error(`ChatSurface message ${messageId} DOM identity diverged before content reconciliation`));
        }
        const liveContent = record.contentElement;
        assertLiveContent(record, liveContent);

        const messages = getMessages();
        const structure = getStructure();
        if (
            structure.keys[messageId] !== record.mountKey
            || keyOf(messages[messageId]) !== record.mountKey
        ) {
            throw setFault(new Error(`ChatSurface message ${messageId} changed structure before content reconciliation`));
        }
        try {
            assertActiveRuntimeSources([{ record }]);
        } catch (error) {
            discard([transaction.content]);
            throw setFault(error);
        }

        const nextLease = createContentLease();
        /** @type {any[]} */
        const candidates = [];
        const prepared = [{
            record,
            context: createDetachedParticipantContext(record, transaction.content),
            contentRoot: null,
            contentParent: transaction.content.parentNode,
            candidates,
            sourceOwners: new Map(),
        }];
        function discardPrepared() {
            discard([
                ...candidates.map(candidate => candidate.source),
                transaction.content,
            ]);
        }
        try {
            if (notifyParticipants) {
                participants.prepare(prepared);
            }
        } catch (error) {
            nextLease.close('content-preparation-failed');
            discardPrepared();
            throw setFault(error);
        }

        try {
            record.contentLease.close('content-update');
            assertLiveContent(record, liveContent);
            participants.assertPrepared(prepared);
        } catch (error) {
            nextLease.close('content-commit-failed');
            discardPrepared();
            throw setFault(error);
        }
        let committedContent;
        try {
            committedContent = transaction.commit();
        } catch (error) {
            nextLease.close('content-commit-failed');
            discardPrepared();
            throw setFault(error);
        }
        if (committedContent !== liveContent) {
            nextLease.close('content-commit-failed');
            discardPrepared();
            throw setFault(new Error('ChatSurface content transaction must synchronously return the exact live .mes_text'));
        }
        record.runtimeSources = Object.freeze(candidates.map(candidate => candidate.source));
        record.message = messages[messageId];
        record.contentLease = nextLease;
        assertLiveContent(record, liveContent);

        if (notifyParticipants) {
            try {
                participants.connected(prepared, ['didCommitContent']);
                runtimeAdmission.register(participants.runtimeCandidates(prepared));
                assertLiveContent(record, liveContent);
            } catch (error) {
                throw setFault(error);
            }
        }
        return record.element;
    }

    return Object.freeze({ update });
}
