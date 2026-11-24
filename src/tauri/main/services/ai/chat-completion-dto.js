// @ts-check

import { mustGetSillyTavernContext } from '../../adapters/st/active-chat-ref.js';
import { createRandomId } from '../../kernel/random-id.js';

const OPENCODE_STABLE_CHAT_ID_FIELD = '_tauritavern_stable_chat_id';

/** @param {Record<string, unknown>} payload */
export function getChatCompletionSource(payload) {
    return String(payload.chat_completion_source || '').trim().toLowerCase();
}

/**
 * @param {Record<string, unknown>} payload
 * @returns {Promise<Record<string, unknown>>}
 */
export async function createChatCompletionDto(payload) {
    if (getChatCompletionSource(payload) !== 'opencode') {
        return payload;
    }

    return { ...payload, [OPENCODE_STABLE_CHAT_ID_FIELD]: await getOpenCodeStableChatId() };
}

async function getOpenCodeStableChatId() {
    if (!mustGetSillyTavernContext().getCurrentChatId()) {
        return createRandomId();
    }

    const chatApi = (/** @type {Window} */ (/** @type {unknown} */ (globalThis))).__TAURITAVERN__?.api?.chat;
    if (!chatApi) {
        throw new Error('TauriTavern host ABI is not initialized');
    }
    return chatApi.current.handle().stableId();
}
