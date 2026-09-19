import { StrictMode } from 'react';
import { createRoot } from 'react-dom/client';

import { reportAgentSystemError, requireAgentApi, waitForHostReady } from './host-api';
import { translateAgentSystem as tr } from './i18n';
import { mountChatInputAgentToggle } from './chat-input-toggle';
import { mountEmbeddedAssetButtons } from './embedded-assets-buttons';
import { mountAgentRunTimelinePanel } from './run-timeline-panel';
import { mountSkillManagerSettingsPanel } from './skill-manager/settings-entry';
import { openAgentSystemPanel } from './panel-popup';
import { loadSettings, patchSettings, subscribeSettings } from './settings-store';
import { startModelTargetLlmConnectionSync, syncSavedModelTargetLlmConnections } from './model-target-connection';
import { subscribeAgentProfilesChanged } from '../../../tauritavern/agent/agent-profile-events.js';
import { AgentSystemEntryApp } from './AgentSystemEntryApp';
import { createAgentSystemEntryController } from './AgentSystemEntryController';

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
        loadSettings,
        patchSettings,
        subscribeSettings,
        listProfiles: async () => {
            const result = await requireAgentApi().profiles.list();
            return Array.isArray(result?.profiles) ? result.profiles : [];
        },
        subscribeProfilesChanged: subscribeAgentProfilesChanged,
        notifyError: reportAgentSystemError,
        notifyWarning: (message) => window.toastr?.warning?.(message),
        tr,
    });

    createRoot(mount).render(
        <StrictMode>
            <AgentSystemEntryApp controller={controller} tr={tr} onOpenPanel={openAgentSystemPanel} />
        </StrictMode>,
    );

    const entryInitialization = controller.init().catch((error) => {
        reportAgentSystemError(error);
        throw error;
    });
    mountSkillManagerSettingsPanel();
    mountEmbeddedAssetButtons();
    await Promise.all([
        entryInitialization,
        mountChatInputAgentToggle(),
        mountAgentRunTimelinePanel(),
    ]);
}

// Top-level await propagates startup failures after every independent mount
// has been started in contract order.
await mountAgentSystem();
