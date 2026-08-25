import { StrictMode } from 'react';
import { createRoot } from 'react-dom/client';

import { AgentSystemPanelApp } from './AgentSystemPanelApp';
import { createAgentSystemPanelController } from './AgentSystemPanelController';
import { CHAT_COMPLETION_PRESET_API_ID } from './AgentSystemPanelContract';
import { createRunHistoryController, type RunHistoryListInput } from './RunHistoryController';
import { createRunRetentionController } from './RunRetentionController';
import { confirmAction, errorText, requireAgentApi, requireHostApi, requireSillyTavernContext } from './host-api.js';
import { translateAgentSystem as tr } from './i18n.js';
import {
    listSavedModelTargets,
    saveModelTargetAsLlmConnection,
    subscribeModelTargetChanges,
} from './model-target-connection.js';
import { openAgentRunTimelineDialog } from './run-timeline-panel';
import { loadSettings, patchSettings } from './settings-store.js';
import { downloadBlobWithRuntime } from '../../../file-export.js';
import { subscribeAgentProfilesChanged } from '../../../tauritavern/agent/agent-profile-events.js';
import { subscribeLlmConnectionsChanged } from '../../../tauritavern/agent/llm-connection-events.js';

let activePanel: { dialog: HTMLDialogElement; dispose: () => void } | null = null;

type SillyTavernPresetManager = {
    getAllPresets: () => string[];
    findPreset: (name: string) => unknown;
};

function listPresetOptions(): string[] {
    const context = requireSillyTavernContext() as {
        getPresetManager?: (apiId: string) => SillyTavernPresetManager | null | undefined;
    };
    const manager = context.getPresetManager?.(CHAT_COMPLETION_PRESET_API_ID);
    if (!manager) {
        throw new Error(tr('presetManagerUnavailable'));
    }
    return manager
        .getAllPresets()
        .map((name) => String(name || '').trim())
        .filter((name) => name && manager.findPreset(name) !== 'gui')
        .sort((a, b) => a.localeCompare(b));
}

async function currentChatRunFilter(): Promise<{ chatRef: TauriTavernChatRef; stableChatId: string }> {
    const chat = requireHostApi('chat') as TauriTavernChatApi;
    const chatRef = chat.current.ref();
    if (!plainObject(chatRef)) {
        throw new Error('agent.run_history_current_chat_invalid: current chat ref must be an object');
    }
    const stableChatIdValue = await chat.current.handle().stableId();
    if (typeof stableChatIdValue !== 'string') {
        throw new Error('agent.run_history_current_chat_invalid: stableChatId must be a string');
    }
    const stableChatId = stableChatIdValue.trim();
    if (!stableChatId) {
        throw new Error('agent.run_history_current_chat_invalid: stableChatId is required');
    }
    return { chatRef, stableChatId };
}

function plainObject(value: unknown): value is Record<string, unknown> {
    if (!value || typeof value !== 'object' || Array.isArray(value)) {
        return false;
    }
    const prototype = Object.getPrototypeOf(value) as unknown;
    return prototype === Object.prototype || prototype === null;
}

function requireRetentionApi(): TauriTavernAgentRetentionApi {
    const agent = requireHostApi('agent') as TauriTavernAgentApi;
    const api = agent.retention;
    if (typeof api?.readSettings !== 'function'
        || typeof api.updateSettings !== 'function'
        || typeof api.planPrune !== 'function'
        || typeof api.applyPrune !== 'function') {
        throw new Error(tr('hostAgentRetentionApiUnavailable'));
    }
    return api;
}

export function openAgentSystemPanel(): void {
    if (activePanel?.dialog.open) {
        activePanel.dialog.focus();
        return;
    }
    if (typeof HTMLDialogElement === 'undefined') {
        throw new Error(tr('agentSystemElementUnsupported'));
    }

    const dialog = document.createElement('dialog');
    if (typeof dialog.showModal !== 'function') {
        throw new Error(tr('agentSystemDialogUnsupported'));
    }
    dialog.className = 'ttas-dialog';
    dialog.setAttribute('data-tt-mobile-surface', 'fullscreen-window');
    const mount = document.createElement('div');
    mount.className = 'ttas-popup-mount';
    dialog.appendChild(mount);
    document.body.appendChild(dialog);

    const runHistory = createRunHistoryController({
        listRuns: (input: RunHistoryListInput) => {
            const agent = requireHostApi('agent') as TauriTavernAgentApi;
            return agent.listRuns(input);
        },
        currentChatRunFilter,
        openRun: (run) => {
            try {
                openAgentRunTimelineDialog(run);
            } catch (error) {
                console.error('[AgentSystem] Failed to open Agent run timeline', error);
                window.toastr?.error?.(errorText(error), tr('agentSystem'));
            }
        },
    });

    const runRetention = createRunRetentionController({
        getRetentionApi: requireRetentionApi,
        confirmAction: (message) => confirmAction(message),
        notifySuccess: (message) => window.toastr?.success?.(message, tr('agentSystem')),
        notifyWarning: (message) => window.toastr?.warning?.(message, tr('agentSystem')),
        tr,
    });

    const controller = createAgentSystemPanelController({
        loadSettings: () => loadSettings(),
        patchSettings: (current, patch) => patchSettings(current, patch),
        getProfilesApi: () => requireAgentApi().profiles,
        listTools: async () => {
            const api = requireAgentApi().tools;
            if (typeof api?.list !== 'function') {
                throw new Error(tr('hostAgentToolApiUnavailable'));
            }
            const result = await api.list();
            return {
                tools: result.tools,
                diagnostics: Array.isArray(result.diagnostics) ? result.diagnostics : [],
            };
        },
        listPresetOptions,
        listModelTargets: () => listSavedModelTargets(),
        saveModelTargetConnection: (target) => saveModelTargetAsLlmConnection(target),
        subscribeProfilesChanged: (listener) => subscribeAgentProfilesChanged(listener),
        subscribeModelTargetsChanged: (listener) => subscribeModelTargetChanges(listener),
        subscribeLlmConnectionsChanged: (listener) => subscribeLlmConnectionsChanged(listener),
        confirmAction: (message) => confirmAction(message),
        downloadBlob: (blob, fileName) => downloadBlobWithRuntime(blob, fileName, {
            fallbackName: 'agent-profile.json',
        }),
        notifyError: (error) => {
            console.error('[AgentSystem]', error);
            window.toastr?.error?.(errorText(error));
        },
        notifyWarning: (message) => window.toastr?.warning?.(message),
        notifySuccess: (message) => window.toastr?.success?.(message),
        onRunsTabActivated: () => {
            void runHistory.refresh();
            void runRetention.refresh();
        },
        tr,
    });

    const root = createRoot(mount);
    let disposed = false;
    const cleanup = () => {
        if (disposed) {
            return;
        }
        disposed = true;
        controller.dispose();
        runHistory.dispose();
        runRetention.dispose();
        root.unmount();
        dialog.remove();
        if (activePanel?.dialog === dialog) {
            activePanel = null;
        }
    };
    dialog.addEventListener('close', cleanup, { once: true });
    dialog.addEventListener('cancel', (event) => {
        event.preventDefault();
        dialog.close();
    });

    root.render(
        <StrictMode>
            <AgentSystemPanelApp
                controller={controller}
                runHistory={runHistory}
                runRetention={runRetention}
                tr={tr}
                onRequestClose={() => dialog.close()}
            />
        </StrictMode>,
    );
    activePanel = { dialog, dispose: cleanup };

    try {
        dialog.showModal();
    } catch (error) {
        cleanup();
        throw error;
    }

    // Init is started once here by the composition root (never inside a React
    // effect). Failures are already reported by the controller; rethrow async
    // so the dev-log capture still observes them, matching the Vue panel.
    void controller.init().catch((error: unknown) => {
        queueMicrotask(() => {
            throw error;
        });
    });
}
