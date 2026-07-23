// @ts-check

import { createResourceLease } from './resource-lease.js';

/** @param {HTMLElement} element */
export function requireMessageContent(element) {
    const content = element.querySelector('.mes_text');
    if (!(content instanceof HTMLElement)) {
        throw new Error('ChatSurface message is missing .mes_text');
    }
    return content;
}

/** @param {any[]} entries @param {boolean} connected */
export function assertResidencyContents(entries, connected) {
    for (const { record } of entries) {
        try {
            const content = requireMessageContent(record.element);
            if (
                content !== record.contentElement
                || content.parentElement !== record.contentParent
                || (connected && !content.isConnected)
            ) {
                throw new Error('content root changed');
            }
        } catch (cause) {
            const error = /** @type {Error & { cause?: unknown }} */ (
                new Error(`ChatSurface message ${record.messageId} content ownership diverged`)
            );
            error.cause = cause;
            throw error;
        }
    }
}

/** @param {any[]} entries */
export function assertActiveRuntimeSources(entries) {
    for (const { record } of entries) {
        for (const source of record.runtimeSources) {
            if (!source.isConnected || !record.contentElement.contains(source)) {
                throw new Error(`ChatSurface message ${record.messageId} runtime source ownership diverged`);
            }
        }
    }
}

/** @param {any[]} entries */
export function assertReleasedRuntimeSources(entries) {
    for (const { record } of entries) {
        for (const source of record.runtimeSources) {
            if (!record.contentElement.contains(source) && source.parentNode !== null) {
                throw new Error(`ChatSurface message ${record.messageId} cleanup exported a runtime source`);
            }
        }
    }
}

/** @param {any} residencies @param {any} projection @param {any} domAdapter */
export function assertCommittedResidencies(residencies, projection, domAdapter) {
    if (residencies.size !== projection.indices.length) {
        throw new Error('ChatSurface projection and residency counts diverged');
    }
    const desired = projection.indices.map(/** @param {number} messageId */ messageId => {
        const record = residencies.getByMessageId(messageId);
        if (!record) {
            throw new Error(`ChatSurface projection is missing residency ${messageId}`);
        }
        return { messageId, element: record.element, record };
    });
    domAdapter.assertCommitted(desired);
    assertResidencyContents(desired, true);
    assertActiveRuntimeSources(desired);
}

function createMountLease() {
    return createResourceLease();
}

export function createContentLease() {
    return createResourceLease();
}

/** @param {{ mountKey: string; messageId: number; message: any; element: HTMLElement; content: HTMLElement }} input */
export function createMessageResidency({ mountKey, messageId, message, element, content }) {
    return {
        mountKey,
        messageId,
        message,
        element,
        contentElement: content,
        contentParent: content.parentElement,
        runtimeSources: Object.freeze([]),
        mountLease: createMountLease(),
        contentLease: createContentLease(),
    };
}

/**
 * Content resources are always released before root-mount resources. Both
 * scopes are attempted even if one cleanup fails.
 *
 * @param {any} record
 * @param {string} reason
 */
export function closeMessageResidency(record, reason) {
    /** @type {unknown} */
    let firstFailure;
    for (const lease of [record.contentLease, record.mountLease]) {
        try {
            lease.close(reason);
        } catch (error) {
            firstFailure ??= error;
        }
    }
    if (firstFailure !== undefined) {
        throw firstFailure;
    }
}

/** @param {any[]} records @param {string} reason */
export function closeMessageResidencies(records, reason) {
    /** @type {unknown} */
    let firstFailure;
    for (const record of records) {
        try {
            closeMessageResidency(record, reason);
        } catch (error) {
            firstFailure ??= error;
        }
    }
    if (firstFailure !== undefined) {
        throw firstFailure;
    }
}
