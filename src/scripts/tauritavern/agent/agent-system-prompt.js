export async function loadResolvedAgentSystemPrompt(profileId) {
    const profileApi = window.__TAURITAVERN__?.api?.agent?.profiles;
    if (typeof profileApi?.resolveSystemPrompt !== 'function') {
        throw new Error('agent.profile_api_unavailable: TauriTavern Agent profile API is unavailable');
    }

    const normalizedProfileId = normalizeOptionalProfileId(profileId);
    const result = await profileApi.resolveSystemPrompt({
        ...(normalizedProfileId ? { profileId: normalizedProfileId } : {}),
    });
    return normalizeAgentSystemPrompt(result?.agentSystemPrompt);
}

export function normalizeAgentSystemPrompt(value) {
    const agentSystemPrompt = String(value ?? '');
    if (!agentSystemPrompt.trim()) {
        throw new Error('agent.system_prompt_required: Agent Mode requires a resolved Agent system prompt');
    }
    return agentSystemPrompt;
}

function normalizeOptionalProfileId(value) {
    const profileId = String(value || '').trim();
    return profileId || undefined;
}
