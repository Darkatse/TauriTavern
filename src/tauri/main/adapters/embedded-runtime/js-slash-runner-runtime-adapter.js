// @ts-check

import { fnv1a32 } from '../../kernel/hash-utils.js';
import { EmbeddedRuntimeKind } from '../../services/embedded-runtime/runtime-kinds.js';
import { createManagedIframeSlot } from './managed-iframe-slot.js';

/**
 * @typedef {import('../../services/embedded-runtime/embedded-runtime-manager.js').createEmbeddedRuntimeManager} createEmbeddedRuntimeManager
 * @typedef {ReturnType<createEmbeddedRuntimeManager>} EmbeddedRuntimeManager
 */

const TH_RENDER_SELECTOR = '.TH-render';
let nextWrapperUid = 0;
const PLACEHOLDER_SELECTOR = '.tt-runtime-placeholder';

/**
 * @param {HTMLElement} wrapper
 */
function ensureWrapperUid(wrapper) {
    if (wrapper.dataset.ttEmbeddedRuntimeUid) {
        return wrapper.dataset.ttEmbeddedRuntimeUid;
    }

    nextWrapperUid += 1;
    wrapper.dataset.ttEmbeddedRuntimeUid = String(nextWrapperUid);
    return wrapper.dataset.ttEmbeddedRuntimeUid;
}

/**
 * @param {HTMLElement} wrapper
 */
function getMessageIdForWrapper(wrapper) {
    const message = wrapper.closest('.mes');
    if (!message) {
        return null;
    }
    const messageId = String(message.getAttribute('mesid') || '').trim();
    return messageId ? messageId : null;
}

/**
 * @param {HTMLElement} wrapper
 */
function getWrapperSignature(wrapper) {
    const code = wrapper.querySelector('pre code');
    if (code instanceof HTMLElement) {
        const text = String(code.textContent || '').trim();
        if (text) {
            return text;
        }
    }

    const iframe = wrapper.querySelector('iframe');
    if (iframe instanceof HTMLIFrameElement) {
        const srcdoc = String(iframe.srcdoc || '').trim();
        if (srcdoc) {
            return srcdoc;
        }
        const src = String(iframe.src || '').trim();
        if (src) {
            return src;
        }
    }

    return null;
}

/**
 * @param {HTMLElement} wrapper
 */
function findManagedSlotId(wrapper) {
    const id = String(wrapper.dataset.ttRuntimeSlotId || '').trim();
    return id ? id : null;
}

/**
 * @param {EmbeddedRuntimeManager} manager
 * @param {HTMLElement} wrapper
 */
function registerWrapper(manager, wrapper) {
    if (!(wrapper instanceof HTMLElement)) {
        return;
    }

    const messageId = getMessageIdForWrapper(wrapper);
    if (!messageId) {
        return;
    }

    const iframe = wrapper.querySelector('iframe');
    if (!(iframe instanceof HTMLIFrameElement)) {
        return;
    }

    if (findManagedSlotId(wrapper)) {
        return;
    }

    const signature = getWrapperSignature(wrapper) || '';
    if (!signature) {
        return;
    }

    const key = fnv1a32(signature);
    const uid = ensureWrapperUid(wrapper);
    const slotId = `jsr:${messageId}:${key}:${uid}`;

    const parkDelayMs = manager.profile === 'mobile-safe' ? 800 : 1500;
    const slot = createManagedIframeSlot({
        id: slotId,
        kind: EmbeddedRuntimeKind.JsrHtmlRender,
        host: wrapper,
        priority: 0,
        weight: 10,
        parkDelayMs,
    });

    manager.register(slot);
}

/**
 * @param {EmbeddedRuntimeManager} manager
 * @param {Node} root
 */
function scanForWrappers(manager, root) {
    if (!(root instanceof Element)) {
        return;
    }

    const nearest = root.closest(TH_RENDER_SELECTOR);
    if (nearest instanceof HTMLElement) {
        registerWrapper(manager, nearest);
    }

    const wrappers = root.querySelectorAll(TH_RENDER_SELECTOR);
    for (const wrapper of wrappers) {
        registerWrapper(manager, /** @type {HTMLElement} */ (wrapper));
    }
}

/**
 * @param {EmbeddedRuntimeManager} manager
 * @param {Node} root
 */
function unregisterWrappers(manager, root) {
    if (!(root instanceof Element)) {
        return;
    }

    if (root.matches(TH_RENDER_SELECTOR)) {
        const id = findManagedSlotId(/** @type {HTMLElement} */ (root));
        if (id) {
            manager.unregister(id);
        }
    }

    const wrappers = root.querySelectorAll(TH_RENDER_SELECTOR);
    for (const wrapper of wrappers) {
        const id = findManagedSlotId(/** @type {HTMLElement} */ (wrapper));
        if (id) {
            manager.unregister(id);
        }
    }
}

/**
 * Installs a lightweight adapter that registers JS-Slash-Runner HTML runtimes
 * (wrappers with class `.TH-render`) as managed embedded runtimes.
 *
 * This adapter is intentionally DOM-driven (no dependency on JSR internals),
 * so it can be extended to similar plugins (e.g. LittleWhiteBox) by adding
 * additional wrapper detectors.
 *
 * @param {{ manager: EmbeddedRuntimeManager }} options
 */
export function installJsSlashRunnerRuntimeAdapter({ manager }) {
    if (!manager) {
        throw new Error('installJsSlashRunnerRuntimeAdapter requires manager');
    }

    const chat = document.querySelector('#chat');
    if (!(chat instanceof HTMLElement)) {
        throw new Error('installJsSlashRunnerRuntimeAdapter: #chat not found');
    }

    scanForWrappers(manager, chat);

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

        const host = placeholder.closest('[data-tt-runtime-slot-id]');
        const id = host instanceof HTMLElement ? String(host.dataset.ttRuntimeSlotId || '').trim() : '';
        if (!id) {
            return;
        }

        manager.touch(id);
    };

    const observer = new MutationObserver((records) => {
        for (const record of records) {
            for (const addedNode of record.addedNodes) {
                scanForWrappers(manager, addedNode);
            }
            for (const removedNode of record.removedNodes) {
                unregisterWrappers(manager, removedNode);
            }
        }
    });

    observer.observe(chat, { childList: true, subtree: true });
    chat.addEventListener('click', onClick, true);

    return {
        dispose: () => {
            observer.disconnect();
            chat.removeEventListener('click', onClick, true);
        },
    };
}
