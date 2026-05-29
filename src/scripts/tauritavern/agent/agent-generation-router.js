import { loadAgentSystemSettings } from './agent-system-settings.js';
import { agentContextPolicyForProfile } from './agent-context-policy.js';
import { loadResolvedAgentSystemPrompt } from './agent-system-prompt.js';

const AGENT_GENERATION_TYPES = new Set(['normal', 'regenerate', 'swipe']);

export async function getAgentGenerationOptions({
    generationType,
    isSlashCommand = false,
    mainApi,
    selectedGroup,
} = {}) {
    const settings = await loadAgentSystemSettings();
    if (!settings.agentModeEnabled || isSlashCommand) {
        return {};
    }

    const type = normalizeGenerationType(generationType);
    assertAgentGenerationSupported({
        generationType: type,
        mainApi,
        selectedGroup,
    });

    const selectedProfile = await loadDirectRunnableProfile(settings.selectedProfileId);
    const agentSystemPrompt = await loadResolvedAgentSystemPrompt(settings.selectedProfileId);

    return {
        agentMode: true,
        agentProfileId: settings.selectedProfileId,
        agentContextPolicy: agentContextPolicyForProfile(selectedProfile),
        agentSystemPrompt,
    };
}

async function loadDirectRunnableProfile(profileId) {
    const normalizedProfileId = String(profileId || '').trim();
    if (!normalizedProfileId) {
        throw new Error('agent.profile_id_required: Agent Mode requires a selected Agent profile');
    }
    const profileApi = window.__TAURITAVERN__?.api?.agent?.profiles;
    if (typeof profileApi?.load !== 'function') {
        throw new Error('agent.profile_api_unavailable: TauriTavern Agent profile API is unavailable');
    }
    const result = await profileApi.load({ profileId: normalizedProfileId });
    const profile = result?.profile;
    if (!profile) {
        throw new Error(`agent.profile_not_found: Agent profile not found: ${normalizedProfileId}`);
    }
    if (profile.run?.directRunnable === false) {
        throw new Error(`agent.profile_not_direct_runnable: Agent profile '${normalizedProfileId}' can only run as a delegated SubAgent`);
    }
    return profile;
}

function normalizeGenerationType(value) {
    return String(value || 'normal').trim() || 'normal';
}

function assertAgentGenerationSupported({ generationType, mainApi, selectedGroup }) {
    if (selectedGroup) {
        throw new Error('agent.group_chat_unsupported: Agent Mode does not support group chats yet');
    }
    if (mainApi !== 'openai') {
        throw new Error('agent.chat_completion_required: Agent Mode currently requires the OpenAI/chat-completion path');
    }
    if (!AGENT_GENERATION_TYPES.has(generationType)) {
        throw new Error(`agent.generation_type_unsupported: Agent Mode currently supports normal, regenerate, and swipe generation, got ${generationType}`);
    }
}
