import { StrictMode } from 'react';
import { createRoot } from 'react-dom/client';

import { downloadBlobWithRuntime } from '../../../../file-export.js';
import { isAndroidRuntime, isIosRuntime } from '../../../../util/mobile-runtime.js';
import { subscribeAgentProfilesChanged } from '../../../../tauritavern/agent/agent-profile-events.js';
import { confirmAction, errorText, requireAgentApi, requireSillyTavernContext, requireSkillApi } from '../host-api.js';
import { translateAgentSystem as tr, translateSkillInstallAction } from '../i18n.js';
import { loadSettings, subscribeSettings } from '../settings-store.js';
import { SkillManager } from './SkillManager';
import type { SkillHostContext, SkillManagerDeps } from './SkillManagerContract';
import { createSkillManagerController } from './SkillManagerController';
import {
    syncSkillDeletePortability,
    syncSkillInstallPortability,
    syncSkillMovePortability,
    syncSkillWritePortability,
} from './embedded-skill-sync.js';

const EXTENSIONS_BLOCK_ID = 'rm_extensions_block';
const AGENT_SYSTEM_CONTAINER_ID = 'agent_system_container';
const SKILL_MANAGER_CONTAINER_ID = 'skill_manager_container';
const SKILL_MANAGER_MOUNT_ID = 'skill_manager_settings_mount';

export function ensureSkillManagerContainer(): HTMLElement {
    const existing = document.getElementById(SKILL_MANAGER_CONTAINER_ID);
    if (existing instanceof HTMLElement) return existing;
    const block = document.getElementById(EXTENSIONS_BLOCK_ID);
    if (!(block instanceof HTMLElement)) throw new Error(tr('extensionsBlockNotFound'));
    const agentContainer = block.querySelector(`#${AGENT_SYSTEM_CONTAINER_ID}`);
    if (!(agentContainer instanceof HTMLElement)) throw new Error(tr('mountContainerNotFound'));
    const container = document.createElement('div');
    container.id = SKILL_MANAGER_CONTAINER_ID;
    container.className = 'extension_container';
    agentContainer.insertAdjacentElement('afterend', container);
    return container;
}

function reportError(error: unknown): void {
    console.error('[AgentSystem:SkillManager]', error);
    window.toastr?.error?.(errorText(error));
}

function getSkillHostContext(): SkillHostContext {
    const context = requireSillyTavernContext();
    if (typeof context !== 'object' || context === null) throw new Error(tr('sillyTavernContextUnavailable'));
    return context;
}

function createDeps(): SkillManagerDeps {
    return {
        loadSettings,
        subscribeSettings,
        listProfiles: async () => {
            const result = await requireAgentApi().profiles.list();
            return result.profiles;
        },
        subscribeProfilesChanged: subscribeAgentProfilesChanged,
        getHostContext: getSkillHostContext,
        getSkillApi: requireSkillApi,
        confirmAction,
        downloadExport: (blob, fileName, fallbackName) => downloadBlobWithRuntime(blob, fileName, { fallbackName }),
        syncInstallPortability: syncSkillInstallPortability,
        syncMovePortability: syncSkillMovePortability,
        syncWritePortability: syncSkillWritePortability,
        syncDeletePortability: syncSkillDeletePortability,
        supportsDirectoryImport: !isAndroidRuntime() && !isIosRuntime(),
        errorText,
        reportError,
        logError: (message, error) => console.error(`[AgentSystem:SkillManager] ${message}:`, error),
        toastSuccess: message => window.toastr?.success?.(message),
        toastError: message => window.toastr?.error?.(message),
        translateInstallAction: translateSkillInstallAction,
        tr,
    };
}

export function mountSkillManagerSettingsPanel(): void {
    if (document.getElementById(SKILL_MANAGER_MOUNT_ID)) return;
    const mount = document.createElement('div');
    mount.id = SKILL_MANAGER_MOUNT_ID;
    ensureSkillManagerContainer().appendChild(mount);
    const controller = createSkillManagerController(createDeps());
    createRoot(mount).render(
        <StrictMode>
            <div id="skill_manager_settings" className="ttas-root ttas-skill-manager-settings">
                <div className="inline-drawer">
                    <div className="inline-drawer-toggle inline-drawer-header">
                        <b>{tr('skillExtension')}</b>
                        <div className="inline-drawer-icon fa-solid fa-circle-chevron-down down"></div>
                    </div>
                    <div className="inline-drawer-content"><SkillManager controller={controller} tr={tr} /></div>
                </div>
            </div>
        </StrictMode>,
    );
    void controller.init().catch(error => queueMicrotask(() => { throw error; }));
}
