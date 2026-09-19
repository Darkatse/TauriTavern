// @ts-check
/// <reference path="../../global.d.ts" />

/**
 * Finds a canonical chat index, optionally excluding an unfinished swipe.
 * @param {readonly ChatMessage[]} chat
 * @param {{ excludePendingSwipe?: boolean, filter?: ((message: ChatMessage) => boolean) | null }} [options]
 * @returns {number|null}
 */
export function findLastMessageId(chat, { excludePendingSwipe = true, filter = null } = {}) {
    for (let i = chat.length - 1; i >= 0; i--) {
        const message = /** @type {ChatMessage} */ (chat[i]);
        if (excludePendingSwipe && message.swipes && typeof message.swipe_id === 'number'
            && message.swipe_id >= message.swipes.length) {
            continue;
        }
        if (!filter || filter(message)) {
            return i;
        }
    }
    return null;
}

/**
 * Swipe macros include the tail message even while its next swipe is pending.
 * @param {readonly ChatMessage[]} chat
 * @returns {number|null} Number of existing swipes, or null when unavailable.
 */
export function getLastSwipeId(chat) {
    const swipes = chat[chat.length - 1]?.swipes;
    return Array.isArray(swipes) ? swipes.length : null;
}

/**
 * @param {readonly ChatMessage[]} chat
 * @returns {number|null} The tail message's 1-based swipe number, including pending swipes.
 */
export function getCurrentSwipeId(chat) {
    const swipeId = chat[chat.length - 1]?.swipe_id;
    return typeof swipeId === 'number' ? swipeId + 1 : null;
}
