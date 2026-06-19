// @ts-check

export async function resolveStableChatId(chatRef) {
    const chatApi = window.__TAURITAVERN__?.api?.chat;
    if (!chatApi || typeof chatApi.open !== 'function') {
        throw new Error('api.chat is required to resolve stableChatId');
    }

    const handle = chatApi.open(chatRef);
    if (!handle || typeof handle.stableId !== 'function') {
        throw new Error('api.chat.open(ref).stableId is required to resolve stableChatId');
    }

    return String(await handle.stableId()).trim();
}
