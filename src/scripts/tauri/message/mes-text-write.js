// @ts-check

import {
    prepareMesTextHtmlPreservingEmbeddedRuntimes,
} from '../../../tauri/main/adapters/embedded-runtime/message-render-transaction.js';
import { syncElementAttributes } from './mes-text-content.js';
import { isEmbeddedRuntimeTakeoverDisabled } from '../../../tauri/main/services/embedded-runtime/embedded-runtime-profile-state.js';
import { getInstalledChatSurfaceController } from '../../../tauri/main/services/chat-surface/runtime.js';
import { isChatVirtualizationEnabled } from '../../../tauri/main/services/chat-surface/chat-virtualization-state.js';
import { morphdom } from '../../../lib.js';
import { segmentExistingTextInElement } from '../../util/stream-fadein.js';

/** @param {HTMLElement} messageElement @param {{ content: HTMLElement; commit: () => unknown }} transaction @param {boolean} transient */
function commitTransaction(messageElement, transaction, transient) {
    const controller = getInstalledChatSurfaceController();
    if (messageElement.isConnected && controller?.ownsMessageElement(messageElement)) {
        return controller.updateContent(messageElement, transaction, { transient });
    }
    return transaction.commit();
}

/**
 * Replaces `.mes_text` HTML using the active TauriTavern runtime policy.
 *
 * - `embedded_runtime_profile = off`: restore upstream SillyTavern write semantics
 * - otherwise: delegate to the embedded-runtime render transaction
 *
 * @param {HTMLElement} messageElement `.mes` element.
 * @param {string} html New HTML for `.mes_text`.
 * @param {{ frontendSourceHandoffEvent?: string | null }} [options]
 */
export function replaceMesTextHtmlWithRuntimePolicy(messageElement, html, { frontendSourceHandoffEvent = null } = {}) {
    const transaction = prepareMesTextHtmlWithRuntimePolicy(
        messageElement,
        html,
        { frontendSourceHandoffEvent },
    );
    return commitTransaction(messageElement, transaction, false);
}

/**
 * Commits a transient content version after releasing the previous content
 * lease, but deliberately defers decorators and runtimes until final content.
 *
 * @param {HTMLElement} messageElement
 * @param {string} html
 * @param {{ fadeIn?: boolean }} [options]
 */
export function replaceTransientMesTextHtmlWithRuntimePolicy(messageElement, html, { fadeIn = false } = {}) {
    const transaction = prepareMesTextHtmlWithRuntimePolicy(messageElement, html);
    if (!fadeIn) {
        return commitTransaction(messageElement, transaction, true);
    }

    const mesText = messageElement.querySelector('.mes_text');
    if (!(mesText instanceof HTMLElement)) {
        throw new Error('replaceTransientMesTextHtmlWithRuntimePolicy: .mes_text not found');
    }
    segmentExistingTextInElement(transaction.content);
    let committed = false;
    return commitTransaction(messageElement, {
        content: transaction.content,
        commit() {
            if (committed) {
                throw new Error('Chat message content transaction was already committed');
            }
            committed = true;
            morphdom(mesText, transaction.content);
            return mesText;
        },
    }, true);
}

/**
 * Creates a parse-once detached content transaction for ChatSurface.
 *
 * @param {HTMLElement} messageElement `.mes` element.
 * @param {string} html New HTML for `.mes_text`.
 * @param {{ frontendSourceHandoffEvent?: string | null }} [options]
 */
export function prepareMesTextHtmlWithRuntimePolicy(messageElement, html, { frontendSourceHandoffEvent = null } = {}) {
    if (!isChatVirtualizationEnabled() && !isEmbeddedRuntimeTakeoverDisabled()) {
        return prepareMesTextHtmlPreservingEmbeddedRuntimes(messageElement, html, { frontendSourceHandoffEvent });
    }

    if (!(messageElement instanceof HTMLElement)) {
        throw new Error('replaceMesTextHtmlWithRuntimePolicy: messageElement must be an HTMLElement');
    }

    const mesText = messageElement.querySelector('.mes_text');
    if (!(mesText instanceof HTMLElement)) {
        throw new Error('replaceMesTextHtmlWithRuntimePolicy: .mes_text not found');
    }

    const content = /** @type {HTMLElement} */ (mesText.cloneNode(false));
    content.innerHTML = String(html ?? '');
    let committed = false;
    return Object.freeze({
        content,
        commit() {
            if (committed) {
                throw new Error('Chat message content transaction was already committed');
            }
            committed = true;
            syncElementAttributes(mesText, content);
            mesText.replaceChildren(...content.childNodes);
            return mesText;
        },
    });
}
