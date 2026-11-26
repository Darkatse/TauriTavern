// @ts-check

import { mustGetSillyTavernContext } from '../../adapters/st/active-chat-ref.js';
import { createRandomId } from '../../kernel/random-id.js';

const STABLE_CHAT_ID_FIELD = '_tauritavern_stable_chat_id';

/** @param {Record<string, unknown>} payload */
export function getChatCompletionSource(payload) {
    return String(payload.chat_completion_source || '').trim().toLowerCase();
}

/**
 * @param {Record<string, unknown>} payload
 * @returns {Promise<Record<string, unknown>>}
 */
export async function createChatCompletionDto(payload) {
    const stableChatId = await getSessionChatId(payload);
    return stableChatId === null ? payload : { ...payload, [STABLE_CHAT_ID_FIELD]: stableChatId };
}

/**
 * @param {Record<string, unknown>} payload
 * @returns {Promise<string | null>}
 */
async function getSessionChatId(payload) {
    switch (getChatCompletionSource(payload)) {
        case 'opencode':
            return await getCurrentStableChatId() ?? createRandomId();
        case 'fireworks':
            return payload.type === 'quiet' ? null : getCurrentStableChatId();
        default:
            return null;
    }
}

/** @returns {Promise<string | null>} */
async function getCurrentStableChatId() {
    if (!mustGetSillyTavernContext().getCurrentChatId()) {
        return null;
    }

    const chatApi = (/** @type {Window} */ (/** @type {unknown} */ (globalThis))).__TAURITAVERN__?.api?.chat;
    if (!chatApi) {
        throw new Error('TauriTavern host ABI is not initialized');
    }
    return chatApi.current.handle().stableId();
}
