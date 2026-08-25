import { StrictMode } from 'react';
import { createRoot } from 'react-dom/client';

import { errorText, requireAgentApi, waitForHostReady } from './host-api.js';
import { translateAgentSystem as tr } from './i18n.js';
import { mountChatInputAgentToggle } from './chat-input-toggle.js';
import { mountEmbeddedAssetButtons } from './embedded-assets-buttons.js';
import { mountAgentRunTimelinePanel } from './run-timeline-panel';
import { mountSkillManagerSettingsPanel } from './skill-manager/settings-entry';
import { openAgentSystemPanel } from './panel-popup';
import { loadSettings, patchSettings, subscribeSettings } from './settings-store.js';
import { startModelTargetLlmConnectionSync, syncSavedModelTargetLlmConnections } from './model-target-connection.js';
import { subscribeAgentProfilesChanged } from '../../../tauritavern/agent/agent-profile-events.js';
import { AgentSystemEntryApp } from './AgentSystemEntryApp';
import { createAgentSystemEntryController } from './AgentSystemEntryController';

function reportError(error: unknown): void {
    console.error('[AgentSystem]', error);
    window.toastr?.error?.(errorText(error));
}

async function mountAgentSystem(): Promise<void> {
    await waitForHostReady();
    startModelTargetLlmConnectionSync();
    await syncSavedModelTargetLlmConnections();

    const container = document.getElementById('agent_system_container');
    if (!(container instanceof HTMLElement)) {
        throw new Error(tr('mountContainerNotFound'));
    }

    const mount = document.createElement('div');
    mount.id = 'agent_system_mount';
    container.appendChild(mount);

    const controller = createAgentSystemEntryController({
        loadSettings: () => loadSettings(),
        patchSettings: (current, patch) => patchSettings(current, patch),
        subscribeSettings,
        listProfiles: async () => {
            const result = await requireAgentApi().profiles.list();
            return Array.isArray(result?.profiles) ? result.profiles : [];
        },
        subscribeProfilesChanged: subscribeAgentProfilesChanged,
        notifyError: reportError,
        notifyWarning: (message) => window.toastr?.warning?.(message),
        tr,
    });

    createRoot(mount).render(
        <StrictMode>
            <AgentSystemEntryApp controller={controller} tr={tr} onOpenPanel={openAgentSystemPanel} />
        </StrictMode>,
    );

    const entryInitialization = controller.init().catch((error) => {
        reportError(error);
        throw error;
    });
    mountSkillManagerSettingsPanel();
    await Promise.all([
        entryInitialization,
        mountChatInputAgentToggle(),
        mountEmbeddedAssetButtons(),
        mountAgentRunTimelinePanel(),
    ]);
}

// Top-level await propagates startup failures after every independent mount
// has been started in contract order.
await mountAgentSystem();
