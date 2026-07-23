// @ts-check

import { requireMessageContent } from './message-residency.js';

/** @param {string} message @param {unknown} cause */
function errorWithCause(message, cause) {
    const error = /** @type {Error & { cause?: unknown }} */ (new Error(message));
    error.cause = cause;
    return error;
}

/** @param {unknown} value @returns {value is PromiseLike<unknown>} */
function isThenable(value) {
    return Boolean(
        value
        && (typeof value === 'object' || typeof value === 'function')
        && typeof /** @type {{ then?: unknown }} */ (value).then === 'function'
    );
}

/** @param {unknown} result @param {string} label */
function requireNoReturn(result, label) {
    if (isThenable(result)) {
        throw new TypeError(`${label} must be synchronous`);
    }
    if (result !== undefined) {
        throw new TypeError(`${label} must not return a value`);
    }
}

/** @param {unknown} result @param {string} label */
function normalizeDisposable(result, label) {
    if (result === undefined) {
        return null;
    }
    if (isThenable(result)) {
        throw new TypeError(`${label} must return synchronously`);
    }
    if (typeof result === 'function') {
        return result;
    }
    if (result && typeof result === 'object' && typeof /** @type {{ dispose?: unknown }} */ (result).dispose === 'function') {
        return /** @type {{ dispose: () => unknown }} */ (result);
    }
    throw new TypeError(`${label} must return a cleanup function, disposable object, or nothing`);
}

/** @param {any} record @param {HTMLElement} content */
export function createDetachedParticipantContext(record, content) {
    return Object.freeze({
        mesid: record.messageId,
        content,
    });
}

/**
 * Runs the cooperative participant lifecycle. ChatSurface validates ownership
 * at phase boundaries; participants remain responsible for their own cleanup.
 *
 * @param {{ getParticipants: () => readonly any[]; assertHealthy: () => void; root: HTMLElement }} deps
 */
export function createChatSurfaceParticipantLifecycle({ getParticipants, assertHealthy, root }) {
    /** @param {any} item */
    function assertPreparedItem(item) {
        const content = item.context.content;
        if (
            !(content instanceof HTMLElement)
            || !content.matches('.mes_text')
            || content.isConnected
            || content.parentNode !== item.contentParent
            || (item.contentRoot && (
                item.contentRoot.isConnected
                || item.contentRoot.parentNode !== null
                || requireMessageContent(item.contentRoot) !== content
            ))
        ) {
            throw new Error('ChatSurface participant changed detached content ownership');
        }
        for (const candidate of item.candidates) {
            if (candidate.source.isConnected || !content.contains(candidate.source)) {
                throw new Error(`ChatSurface runtime source changed during preparation: ${candidate.participantId}`);
            }
        }
    }

    /** @param {any[]} prepared */
    function assertPrepared(prepared) {
        for (const item of prepared) {
            assertPreparedItem(item);
        }
    }

    /** @param {any[]} prepared */
    function prepare(prepared) {
        for (const item of prepared) {
            item.candidates.length = 0;
            item.sourceOwners.clear();
        }
        assertPrepared(prepared);

        for (const participant of getParticipants()) {
            const hook = participant.prepareContent;
            if (!hook) {
                continue;
            }
            for (const item of prepared) {
                let acceptingClaims = true;
                const claims = Object.freeze({
                    /** @param {unknown} source @param {unknown} activate */
                    claim(source, activate) {
                        if (!acceptingClaims) {
                            throw new Error(`ChatSurface participant ${participant.id} claimed a runtime after prepareContent returned`);
                        }
                        if (!(source instanceof Element) || source === item.context.content || !item.context.content.contains(source)) {
                            throw new Error(`ChatSurface participant ${participant.id} must claim a descendant of message content`);
                        }
                        if (typeof activate !== 'function') {
                            throw new TypeError(`ChatSurface participant ${participant.id} runtime activate must be a function`);
                        }
                        const owner = item.sourceOwners.get(source);
                        if (owner) {
                            throw new Error(`ChatSurface runtime source was claimed by both ${owner} and ${participant.id}`);
                        }
                        item.sourceOwners.set(source, participant.id);
                        item.candidates.push(Object.freeze({ participantId: participant.id, source, activate }));
                    },
                });
                try {
                    requireNoReturn(
                        hook(item.context, claims),
                        `ChatSurface participant ${participant.id}.prepareContent`,
                    );
                    assertHealthy();
                } finally {
                    acceptingClaims = false;
                }
            }
        }
        assertPrepared(prepared);
    }

    /**
     * @param {Array<{ record: any }>} prepared
     * @param {readonly ('didMount' | 'didCommitContent')[]} hookNames
     */
    function connected(prepared, hookNames) {
        for (const hookName of hookNames) {
            for (const participant of getParticipants()) {
                const hook = participant[hookName];
                if (!hook) {
                    continue;
                }
                for (const { record } of prepared) {
                    const lease = hookName === 'didMount' ? record.mountLease : record.contentLease;
                    const disposable = normalizeDisposable(hook(Object.freeze({
                        mesid: record.messageId,
                        element: record.element,
                        content: record.contentElement,
                        signal: lease.signal,
                    })), `ChatSurface participant ${participant.id}.${hookName}`);
                    if (disposable) {
                        lease.add(disposable);
                    }
                    assertHealthy();
                }
            }
        }
    }

    /** @param {any} record @param {any} candidate */
    function assertCandidate(record, candidate) {
        const content = record.contentElement;
        if (
            record.contentLease.signal.aborted
            || record.element.parentElement !== root
            || record.element.getAttribute('mesid') !== String(record.messageId)
            || requireMessageContent(record.element) !== content
            || !candidate.source.isConnected
            || !content.contains(candidate.source)
        ) {
            throw new Error(`ChatSurface runtime source is not live: ${candidate.participantId}`);
        }
    }

    /** @param {any} record @param {any} candidate @param {any} runtimeLease */
    function activateCandidate(record, candidate, runtimeLease) {
        let result;
        try {
            result = candidate.activate(Object.freeze({
                source: candidate.source,
                mesid: record.messageId,
                element: record.element,
                content: record.contentElement,
                signal: runtimeLease.signal,
            }));
        } catch (error) {
            throw errorWithCause(`ChatSurface runtime activation failed: ${candidate.participantId}`, error);
        }
        const disposable = normalizeDisposable(result, `ChatSurface runtime ${candidate.participantId}`);
        if (!disposable) {
            throw new Error(`ChatSurface runtime ${candidate.participantId} must return a disposable`);
        }
        runtimeLease.add(disposable);
        assertHealthy();
    }

    /** @param {Array<{ record: any; candidates: any[] }>} prepared */
    function runtimeCandidates(prepared) {
        return getParticipants().flatMap(participant => prepared.flatMap(({ record, candidates }) => (
            candidates
                .filter(candidate => candidate.participantId === participant.id)
                .map(candidate => Object.freeze({ record, candidate }))
        )));
    }

    return Object.freeze({ prepare, assertPrepared, connected, assertCandidate, activateCandidate, runtimeCandidates });
}
