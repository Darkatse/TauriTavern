import { composeAuthorsNote, isWorldInfoInjection, NOTE_MODULE_NAME } from '../../extension-prompts.js';
import { prepareAgentHistory } from './agent-model-messages.js';

export const DEFAULT_AGENT_CONTEXT_POLICY = Object.freeze({
    initialChatHistoryMessages: -1,
    includeActivatedWorldInfo: true,
});

/** @param {unknown} [value] */
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

/** Returns a fresh working input; the captured input is never mutated. */
export function projectAgentPromptInputs(promptInputs, policy) {
    const resolved = normalizeAgentContextPolicy(policy);
    const { agentMessages, worldInfoExamples, authorsNoteAdditions, ...working } = structuredClone(promptInputs);
    working.messages = agentMessages
        ? prepareAgentHistory(agentMessages, resolved)
        : windowChatHistory(working.messages, resolved.initialChatHistoryMessages);

    let noteAdditions = authorsNoteAdditions;
    if (resolved.includeActivatedWorldInfo) {
        working.messageExamples = [...worldInfoExamples.before, ...working.messageExamples, ...worldInfoExamples.after];
    } else {
        working.worldInfoBefore = '';
        working.worldInfoAfter = '';
        for (const key of Object.keys(working.extensionPrompts)) {
            if (isWorldInfoInjection(key)) delete working.extensionPrompts[key];
        }
        noteAdditions = authorsNoteAdditions?.persona ? { ...authorsNoteAdditions, worldInfo: null } : null;
    }
    if (noteAdditions) {
        working.extensionPrompts[NOTE_MODULE_NAME] = composeAuthorsNote(working.extensionPrompts[NOTE_MODULE_NAME], noteAdditions);
    }
    return working;
}

function windowChatHistory(messages, limit) {
    if (!Array.isArray(messages)) {
        throw new Error('agent.context_history_messages_invalid: messages must be an array');
    }
    // SillyTavern's OpenAI PromptManager raw chat history is latest-first.
    return limit < 0 ? messages : messages.slice(0, limit);
}
