// @ts-check

/**
 * @typedef {'windowed' | 'off'} ChatHistoryModeName
 */

export const CHAT_HISTORY_MODE_WINDOWED = 'windowed';
export const CHAT_HISTORY_MODE_OFF = 'off';

const CHAT_HISTORY_MODE_STORAGE_KEY = 'tt:chatHistoryMode';

/** @type {ChatHistoryModeName | null} */
let cachedBootstrapModeName = null;

/**
 * @param {unknown} value
 * @returns {ChatHistoryModeName}
 */
export function normalizeChatHistoryModeName(value) {
    const modeName = String(value || '').trim();
    if (!modeName) {
        throw new Error('Chat history mode is required');
    }

    if (modeName === CHAT_HISTORY_MODE_WINDOWED || modeName === CHAT_HISTORY_MODE_OFF) {
        return modeName;
    }

    throw new Error(`Unsupported chat history mode: ${modeName}`);
}

export function readStoredChatHistoryModeName() {
    const raw = String(globalThis.localStorage?.getItem(CHAT_HISTORY_MODE_STORAGE_KEY) || '').trim();
    return raw ? normalizeChatHistoryModeName(raw) : null;
}

/**
 * Updates stored value without changing the current session bootstrap cache.
 *
 * @param {ChatHistoryModeName} modeName
 * @returns {ChatHistoryModeName}
 */
export function writeStoredChatHistoryModeName(modeName) {
    const normalized = normalizeChatHistoryModeName(modeName);
    globalThis.localStorage?.setItem(CHAT_HISTORY_MODE_STORAGE_KEY, normalized);
    return normalized;
}

export function getChatHistoryBootstrapModeName() {
    if (cachedBootstrapModeName !== null) {
        return cachedBootstrapModeName;
    }

    const stored = readStoredChatHistoryModeName();
    cachedBootstrapModeName = stored ?? CHAT_HISTORY_MODE_WINDOWED;
    return cachedBootstrapModeName;
}

/**
 * @param {ChatHistoryModeName} modeName
 * @returns {ChatHistoryModeName}
 */
export function setChatHistoryBootstrapModeName(modeName) {
    const normalized = normalizeChatHistoryModeName(modeName);
    globalThis.localStorage?.setItem(CHAT_HISTORY_MODE_STORAGE_KEY, normalized);
    cachedBootstrapModeName = normalized;
    return normalized;
}
