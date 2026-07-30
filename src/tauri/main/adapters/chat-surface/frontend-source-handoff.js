// @ts-check

import { eventSource, event_types } from '../../../../scripts/events.js';

export const FRONTEND_SOURCE_HANDOFF_ATTRIBUTE = 'data-tt-frontend-source-handoff';

const SUPPORTED_EVENTS = new Set([
    event_types.CHAT_CHANGED,
    event_types.CHAT_LOADED,
]);

/** @param {unknown} eventType @returns {asserts eventType is string} */
export function assertFrontendSourceHandoffEvent(eventType) {
    if (typeof eventType !== 'string' || !SUPPORTED_EVENTS.has(eventType)) {
        throw new Error(`Unsupported frontend source handoff event: ${String(eventType)}`);
    }
}

/** @param {unknown} eventType */
export function getFrontendSourceHandoffSelector(eventType) {
    assertFrontendSourceHandoffEvent(eventType);
    return `pre[${FRONTEND_SOURCE_HANDOFF_ATTRIBUTE}="${eventType}"]`;
}

/** @param {Iterable<HTMLElement>} sources @param {unknown} eventType */
export function markFrontendSourceHandoff(sources, eventType) {
    assertFrontendSourceHandoffEvent(eventType);
    for (const source of sources) {
        source.setAttribute(FRONTEND_SOURCE_HANDOFF_ATTRIBUTE, eventType);
    }
}

/**
 * Owns frontend-source cover release independently from the legacy Embedded
 * Runtime so chat-open events cannot race a late APP_READY installation.
 *
 * @param {HTMLElement} root
 */
export function installFrontendSourceHandoff(root) {
    if (!(root instanceof HTMLElement)) {
        throw new TypeError('Frontend source handoff requires a chat root');
    }

    /** @type {Map<HTMLElement, Set<string>>} */
    const awaitingRelease = new Map();
    /** @type {number | null} */
    let releaseFrame = null;
    let disposed = false;

    function releaseAwaiting() {
        for (const [message, eventTypes] of awaitingRelease) {
            for (const eventType of eventTypes) {
                for (const source of message.querySelectorAll(getFrontendSourceHandoffSelector(eventType))) {
                    source.removeAttribute(FRONTEND_SOURCE_HANDOFF_ATTRIBUTE);
                }
            }
        }
        awaitingRelease.clear();
    }

    /** @param {string} eventType */
    function scheduleRelease(eventType) {
        if (disposed) {
            return;
        }
        for (const source of root.querySelectorAll(getFrontendSourceHandoffSelector(eventType))) {
            let message = source;
            while (message.parentElement && message.parentElement !== root) {
                message = message.parentElement;
            }
            if (message instanceof HTMLElement && message.parentElement === root && message.matches('.mes')) {
                const eventTypes = awaitingRelease.get(message) ?? new Set();
                eventTypes.add(eventType);
                awaitingRelease.set(message, eventTypes);
            }
        }
        if (awaitingRelease.size === 0 || releaseFrame !== null) {
            return;
        }
        releaseFrame = requestAnimationFrame(() => {
            releaseFrame = null;
            releaseAwaiting();
        });
    }

    const onChatChanged = () => scheduleRelease(event_types.CHAT_CHANGED);
    const onChatLoaded = () => scheduleRelease(event_types.CHAT_LOADED);
    const moveAfterRenderers = () => {
        if (disposed) {
            return;
        }
        eventSource.makeLast(event_types.CHAT_CHANGED, onChatChanged);
        eventSource.makeLast(event_types.CHAT_LOADED, onChatLoaded);
    };
    moveAfterRenderers();
    eventSource.on(event_types.EXTENSION_SETTINGS_LOADED, moveAfterRenderers);

    return Object.freeze({
        dispose() {
            disposed = true;
            if (releaseFrame !== null) {
                cancelAnimationFrame(releaseFrame);
                releaseFrame = null;
            }
            releaseAwaiting();
            for (const source of root.querySelectorAll(`[${FRONTEND_SOURCE_HANDOFF_ATTRIBUTE}]`)) {
                source.removeAttribute(FRONTEND_SOURCE_HANDOFF_ATTRIBUTE);
            }
            eventSource.removeListener(event_types.CHAT_CHANGED, onChatChanged);
            eventSource.removeListener(event_types.CHAT_LOADED, onChatLoaded);
            eventSource.removeListener(event_types.EXTENSION_SETTINGS_LOADED, moveAfterRenderers);
        },
    });
}
