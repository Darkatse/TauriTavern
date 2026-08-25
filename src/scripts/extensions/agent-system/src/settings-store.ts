import {
    loadAgentSystemSettings,
    patchAgentSystemSettings,
    subscribeAgentSystemSettings,
} from '../../../tauritavern/agent/agent-system-settings.js';

export type AgentSystemSettings = Awaited<ReturnType<typeof loadAgentSystemSettings>>;

export const loadSettings = loadAgentSystemSettings;
export const patchSettings = patchAgentSystemSettings;
export const subscribeSettings = subscribeAgentSystemSettings;
