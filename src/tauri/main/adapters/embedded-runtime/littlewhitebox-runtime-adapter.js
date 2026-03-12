// @ts-check

import { fnv1a32 } from '../../kernel/hash-utils.js';
import { EmbeddedRuntimeKind } from '../../services/embedded-runtime/runtime-kinds.js';
import { installDomEmbeddedRuntimeAdapter } from './dom-runtime-adapter.js';
import { createManagedIframeSlot } from './managed-iframe-slot.js';

/**
 * @typedef {import('../../services/embedded-runtime/embedded-runtime-manager.js').createEmbeddedRuntimeManager} createEmbeddedRuntimeManager
 * @typedef {ReturnType<createEmbeddedRuntimeManager>} EmbeddedRuntimeManager
 */

const XIAOBAIX_WRAPPER_SELECTOR = '.xiaobaix-iframe-wrapper';
let nextWrapperUid = 0;

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
    const pre = wrapper.nextElementSibling;
    if (pre instanceof HTMLPreElement) {
        const hash = String(pre.dataset.xbHash || '').trim();
        if (hash) {
            return `xb:${hash}`;
        }

        const code = pre.querySelector('code');
        if (code instanceof HTMLElement) {
            const text = String(code.textContent || '').trim();
            if (text) {
                return text;
            }
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
    const slotId = `lwb:${messageId}:${key}:${uid}`;

    const parkDelayMs = manager.profile === 'mobile-safe' ? 800 : 1500;
    const slot = createManagedIframeSlot({
        id: slotId,
        kind: EmbeddedRuntimeKind.LittleWhiteBoxHtmlRender,
        host: wrapper,
        priority: 0,
        weight: 10,
        parkDelayMs,
    });

    manager.register(slot);
}

export function createLittleWhiteBoxRuntimeAdapter() {
    return Object.freeze({
        hostSelector: XIAOBAIX_WRAPPER_SELECTOR,
        registerHost: registerWrapper,
    });
}

/**
 * Back-compat single-adapter installer.
 * @param {{ manager: EmbeddedRuntimeManager }} options
 */
export function installLittleWhiteBoxRuntimeAdapter({ manager }) {
    if (!manager) {
        throw new Error('installLittleWhiteBoxRuntimeAdapter requires manager');
    }

    const chat = document.querySelector('#chat');
    if (!(chat instanceof HTMLElement)) {
        throw new Error('installLittleWhiteBoxRuntimeAdapter: #chat not found');
    }

    return installDomEmbeddedRuntimeAdapter({
        manager,
        root: chat,
        adapters: [createLittleWhiteBoxRuntimeAdapter()],
    });
}

