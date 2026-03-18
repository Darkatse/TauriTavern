// @ts-check

import { replaceMesTextHtmlPreservingEmbeddedRuntimes } from '../../../tauri/main/adapters/embedded-runtime/message-render-transaction.js';
import { isEmbeddedRuntimeTakeoverDisabled } from '../../../tauri/main/services/embedded-runtime/embedded-runtime-profile-state.js';

/**
 * Replaces `.mes_text` HTML using the active TauriTavern runtime policy.
 *
 * - `embedded_runtime_profile = off`: restore upstream SillyTavern write semantics
 * - otherwise: delegate to the embedded-runtime render transaction
 *
 * @param {HTMLElement} messageElement `.mes` element.
 * @param {string} html New HTML for `.mes_text`.
 */
export function replaceMesTextHtmlWithRuntimePolicy(messageElement, html) {
    if (!isEmbeddedRuntimeTakeoverDisabled()) {
        replaceMesTextHtmlPreservingEmbeddedRuntimes(messageElement, html);
        return;
    }

    if (!(messageElement instanceof HTMLElement)) {
        throw new Error('replaceMesTextHtmlWithRuntimePolicy: messageElement must be an HTMLElement');
    }

    const mesText = messageElement.querySelector('.mes_text');
    if (!(mesText instanceof HTMLElement)) {
        throw new Error('replaceMesTextHtmlWithRuntimePolicy: .mes_text not found');
    }

    mesText.innerHTML = String(html ?? '');
}
