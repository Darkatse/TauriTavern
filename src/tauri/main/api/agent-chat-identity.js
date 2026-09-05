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

export async function assertCurrentChat(expectedRef, expectedStableChatId = null) {
    const currentRef = window.__TAURITAVERN__?.api?.chat?.current?.ref?.();
    if (sameChatRef(currentRef, expectedRef)) return;

    const expectedStable = String(expectedStableChatId || '').trim();
    if (expectedStable && await resolveStableChatId(currentRef) === expectedStable) return;

    throw new Error('agent.commit_chat_mismatch: active chat changed before commit');
}

function sameChatRef(a, b) {
    if (!a || !b || a.kind !== b.kind) return false;
    if (a.kind === 'character') {
        return String(a.characterId || '') === String(b.characterId || '')
            && String(a.fileName || '') === String(b.fileName || '');
    }
    return String(a.chatId || '') === String(b.chatId || '');
}
