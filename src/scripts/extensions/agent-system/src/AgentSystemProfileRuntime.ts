import { normalizeAgentSystemPrompt } from '../../../tauritavern/agent/agent-system-prompt.js';
import { errorText } from './host-api.js';

export type AgentSystemProfileRuntimeState = {
    resolvedAgentSystemPrompt: string;
    profilePreviewError: string;
    profileHealth: TauriTavernAgentProfileHealth | null;
    profileDiagnosticError: string;
};

export async function loadAgentSystemProfileRuntime(
    profiles: TauriTavernAgentProfilesApi,
    profileId: string,
): Promise<AgentSystemProfileRuntimeState> {
    const [diagnostic, preview] = await Promise.allSettled([
        profiles.diagnose({ profileId }),
        profiles.resolveSystemPrompt({ profileId }),
    ]);
    return {
        profileHealth: diagnostic.status === 'fulfilled' ? diagnostic.value || null : null,
        profileDiagnosticError: diagnostic.status === 'rejected' ? errorText(diagnostic.reason) : '',
        resolvedAgentSystemPrompt: preview.status === 'fulfilled'
            ? normalizeAgentSystemPrompt(preview.value?.agentSystemPrompt)
            : '',
        profilePreviewError: preview.status === 'rejected' ? errorText(preview.reason) : '',
    };
}
