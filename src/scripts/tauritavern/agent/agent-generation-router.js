import { loadAgentSystemSettings } from './agent-system-settings.js';
import { loadAgentContextPolicy } from './agent-context-policy.js';
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

    const [agentContextPolicy, agentSystemPrompt] = await Promise.all([
        loadAgentContextPolicy(settings.selectedProfileId),
        loadResolvedAgentSystemPrompt(settings.selectedProfileId),
    ]);

    return {
        agentMode: true,
        agentProfileId: settings.selectedProfileId,
        agentContextPolicy,
        agentSystemPrompt,
    };
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
