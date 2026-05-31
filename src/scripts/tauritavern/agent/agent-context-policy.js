export const DEFAULT_AGENT_CONTEXT_POLICY = Object.freeze({
    initialChatHistoryMessages: -1,
    includeActivatedWorldInfo: true,
});

export function normalizeAgentContextPolicy(value = {}) {
    const source = value || {};
    const initialChatHistoryMessages = Number(source.initialChatHistoryMessages ?? DEFAULT_AGENT_CONTEXT_POLICY.initialChatHistoryMessages);

    if (!Number.isInteger(initialChatHistoryMessages)) {
        throw new Error('agent.context_history_invalid: initialChatHistoryMessages must be negative for full history, zero for no initial history, or positive for a recent-message window');
    }

    return {
        initialChatHistoryMessages: initialChatHistoryMessages < 0 ? -1 : initialChatHistoryMessages,
        includeActivatedWorldInfo: source.includeActivatedWorldInfo !== false,
    };
}

export function agentContextPolicyForProfile(profile) {
    return normalizeAgentContextPolicy(profile?.context);
}

export async function loadAgentContextPolicy(profileId) {
    const normalizedProfileId = String(profileId || '').trim();
    if (!normalizedProfileId) {
        return { ...DEFAULT_AGENT_CONTEXT_POLICY };
    }

    const profileApi = window.__TAURITAVERN__?.api?.agent?.profiles;
    if (typeof profileApi?.load !== 'function') {
        throw new Error('agent.profile_api_unavailable: TauriTavern Agent profile API is unavailable');
    }

    const result = await profileApi.load({ profileId: normalizedProfileId });
    if (!result?.profile) {
        throw new Error(`agent.profile_not_found: Agent profile not found: ${normalizedProfileId}`);
    }

    return agentContextPolicyForProfile(result.profile);
}

export function applyInitialChatHistoryPolicy(coreChat, policy) {
    if (!Array.isArray(coreChat)) {
        throw new Error('agent.context_history_messages_invalid: messages must be an array');
    }

    const resolved = normalizeAgentContextPolicy(policy);
    if (resolved.initialChatHistoryMessages < 0) {
        return coreChat;
    }
    if (resolved.initialChatHistoryMessages === 0) {
        return [];
    }

    // SillyTavern's OpenAI PromptManager raw chat history is latest-first.
    // Positive Agent windows therefore keep the front of the array.
    return coreChat.slice(0, resolved.initialChatHistoryMessages);
}

export function materializeInitialChatHistoryMessages(coreChat, policy) {
    // PromptManager assembly mutates history while injecting prompts and
    // reversing into provider order; frozen Agent input must stay reusable.
    return structuredClone(applyInitialChatHistoryPolicy(coreChat, policy));
}
