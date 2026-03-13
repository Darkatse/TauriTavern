// @ts-check

import { eventSource, event_types } from '../../../../scripts/events.js';
import { scanForHosts, unregisterSlotsInSubtree } from './dom-runtime-adapter.js';
import { createJsSlashRunnerRuntimeAdapter } from './js-slash-runner-runtime-adapter.js';
import { createLittleWhiteBoxRuntimeAdapter } from './littlewhitebox-runtime-adapter.js';
import { parkManagedIframe } from './managed-iframe-parking-lot.js';

/**
 * @typedef {import('../../services/embedded-runtime/embedded-runtime-manager.js').createEmbeddedRuntimeManager} createEmbeddedRuntimeManager
 * @typedef {ReturnType<createEmbeddedRuntimeManager>} EmbeddedRuntimeManager
 */

const PLACEHOLDER_SELECTOR = '.tt-runtime-placeholder';
const SLOT_ID_SELECTOR = '[data-tt-runtime-slot-id]';

/**
 * @param {unknown} value
 */
function toMessageId(value) {
    const n = Number(value);
    return Number.isInteger(n) && n >= 0 ? n : null;
}

/**
 * Installs embedded runtime detectors for chat-scoped render extensions.
 *
 * @param {{ manager: EmbeddedRuntimeManager }} options
 */
export function installChatEmbeddedRuntimeAdapters({ manager }) {
    if (!manager) {
        throw new Error('installChatEmbeddedRuntimeAdapters requires manager');
    }

    const chat = document.querySelector('#chat');
    if (!(chat instanceof HTMLElement)) {
        throw new Error('installChatEmbeddedRuntimeAdapters: #chat not found');
    }

    const adapters = [
        createJsSlashRunnerRuntimeAdapter(),
        createLittleWhiteBoxRuntimeAdapter(),
    ];

    /** @type {WeakSet<HTMLElement>} */
    let scannedMessages = new WeakSet();

    /** @param {HTMLElement} messageElement */
    const scanMessageElement = (messageElement) => {
        scanForHosts(manager, messageElement, adapters);
        scannedMessages.add(messageElement);
    };

    const scanUnseenMessages = () => {
        for (const el of chat.children) {
            if (!(el instanceof HTMLElement)) {
                continue;
            }
            if (!el.classList.contains('mes')) {
                continue;
            }
            if (scannedMessages.has(el)) {
                continue;
            }
            scanMessageElement(el);
        }
    };

    /**
     * @param {unknown} messageId
     */
    const scanMessageById = (messageId) => {
        const id = toMessageId(messageId);
        if (id === null) {
            return;
        }
        const el = chat.querySelector(`.mes[mesid="${id}"]`);
        if (!(el instanceof HTMLElement)) {
            return;
        }
        scanMessageElement(el);
    };

    /** @param {Event} event */
    const onClick = (event) => {
        const target = event.target;
        if (!(target instanceof Element)) {
            return;
        }

        const placeholder = target.closest(PLACEHOLDER_SELECTOR);
        if (!placeholder) {
            return;
        }

        const host = placeholder.closest(SLOT_ID_SELECTOR);
        const id = host instanceof HTMLElement ? String(host.dataset.ttRuntimeSlotId || '').trim() : '';
        if (!id) {
            return;
        }

        manager.touch(id);
    };

    const observer = new MutationObserver((records) => {
        for (const record of records) {
            const target = record.target;
            const slotHost = target instanceof Element ? target.closest(SLOT_ID_SELECTOR) : null;
            for (const removedNode of record.removedNodes) {
                if (removedNode instanceof HTMLIFrameElement) {
                    if (removedNode.dataset.ttRuntimeManaged === '1') {
                        continue;
                    }

                    if (!(slotHost instanceof HTMLElement) || slotHost.dataset.ttRuntimeMoving === '1') {
                        continue;
                    }

                    const id = String(slotHost.dataset.ttRuntimeSlotId || '').trim();
                    if (!id) {
                        continue;
                    }

                    const { maxSoftParkedIframes, softParkTtlMs } = manager.profileConfig;
                    if (maxSoftParkedIframes > 0) {
                        parkManagedIframe({
                            id,
                            iframe: removedNode,
                            maxIframes: maxSoftParkedIframes,
                            ttlMs: softParkTtlMs,
                        });
                    }

                    unregisterSlotsInSubtree(manager, slotHost);
                    continue;
                }

                if (!(removedNode instanceof HTMLElement)) {
                    continue;
                }
                if (removedNode.dataset.ttRuntimeMoving === '1') {
                    continue;
                }
                if (removedNode.matches(SLOT_ID_SELECTOR) || removedNode.classList.contains('mes')) {
                    unregisterSlotsInSubtree(manager, removedNode);
                }
            }
        }

        for (const record of records) {
            for (const addedNode of record.addedNodes) {
                if (!(addedNode instanceof Element)) {
                    continue;
                }
                if (addedNode instanceof HTMLElement && addedNode.dataset.ttRuntimeMoving === '1') {
                    continue;
                }

                if (addedNode instanceof HTMLIFrameElement || addedNode.tagName === 'IFRAME') {
                    for (const adapter of adapters) {
                        const host = addedNode.closest(adapter.hostSelector);
                        if (host instanceof HTMLElement) {
                            adapter.registerHost(manager, host);
                        }
                    }
                    continue;
                }

                for (const adapter of adapters) {
                    if (addedNode.matches(adapter.hostSelector) && addedNode instanceof HTMLElement) {
                        adapter.registerHost(manager, addedNode);
                    }
                }
            }
        }
    });

    const onChatChanged = () => {
        scannedMessages = new WeakSet();
        scanUnseenMessages();
    };
    const onChatLoaded = () => {
        scanUnseenMessages();
    };
    const onMoreMessagesLoaded = () => {
        scanUnseenMessages();
    };

    /** @param {unknown} messageId */
    const onUserMessageRendered = (messageId) => {
        scanMessageById(messageId);
    };
    /** @param {unknown} messageId */
    const onCharacterMessageRendered = (messageId) => {
        scanMessageById(messageId);
    };
    /** @param {unknown} messageId */
    const onMessageUpdated = (messageId) => {
        scanMessageById(messageId);
    };
    /** @param {unknown} messageId */
    const onMessageSwiped = (messageId) => {
        scanMessageById(messageId);
    };

    scanUnseenMessages();
    observer.observe(chat, { childList: true, subtree: true });
    chat.addEventListener('click', onClick, true);

    eventSource.makeLast(event_types.CHAT_CHANGED, onChatChanged);
    eventSource.makeLast(event_types.CHAT_LOADED, onChatLoaded);
    eventSource.makeLast(event_types.MORE_MESSAGES_LOADED, onMoreMessagesLoaded);

    eventSource.makeLast(event_types.USER_MESSAGE_RENDERED, onUserMessageRendered);
    eventSource.makeLast(event_types.CHARACTER_MESSAGE_RENDERED, onCharacterMessageRendered);
    eventSource.makeLast(event_types.MESSAGE_UPDATED, onMessageUpdated);
    eventSource.makeLast(event_types.MESSAGE_SWIPED, onMessageSwiped);

    return {
        dispose: () => {
            observer.disconnect();
            chat.removeEventListener('click', onClick, true);

            eventSource.removeListener(event_types.CHAT_CHANGED, onChatChanged);
            eventSource.removeListener(event_types.CHAT_LOADED, onChatLoaded);
            eventSource.removeListener(event_types.MORE_MESSAGES_LOADED, onMoreMessagesLoaded);

            eventSource.removeListener(event_types.USER_MESSAGE_RENDERED, onUserMessageRendered);
            eventSource.removeListener(event_types.CHARACTER_MESSAGE_RENDERED, onCharacterMessageRendered);
            eventSource.removeListener(event_types.MESSAGE_UPDATED, onMessageUpdated);
            eventSource.removeListener(event_types.MESSAGE_SWIPED, onMessageSwiped);
        },
    };
}
