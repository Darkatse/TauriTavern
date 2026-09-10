// @ts-check

import { eventSource, event_types } from '../../../../scripts/events.js';
import { t } from '../../../../scripts/i18n.js';
import { getTauriTavernSettings } from '../../../../tauri-bridge.js';
import { activateApiConnectionsSubtreeGates } from '../../adapters/panel-runtime/api-connections-subtree-gates.js';
import { installExtensionsSubtreeGates } from '../../adapters/panel-runtime/extensions-subtree-gates.js';
import { installTopSettingsPanelParking } from '../../adapters/panel-runtime/top-settings-panel-parking.js';
import { createPanelRuntimeService } from './panel-runtime-service.js';
import { validatePanelRuntimeInvariants } from './validate.js';

// Keep in sync with:
// - src/tauri/main/services/panel-runtime/preinstall.js
// - src/scripts/tauri/setting/setting-panel.js
const PANEL_RUNTIME_PROFILE_STORAGE_KEY = 'tt:panelRuntimeProfile';

export function installPanelRuntime() {
    const ready = getTauriTavernSettings().then((settings) => {
        const profileName = String(settings.panel_runtime_profile || 'off').trim();
        localStorage.setItem(PANEL_RUNTIME_PROFILE_STORAGE_KEY, profileName);

        if (profileName === 'off') {
            return null;
        }

        const service = createPanelRuntimeService({ profileName });

        eventSource.on(event_types.APP_READY, () => {
            // eventSource.emit() swallows listener exceptions; without this the fault
            // is invisible and the runtime stays half-installed.
            try {
                activateApiConnectionsSubtreeGates({ manager: service.manager });
                const { pinnedSelectors } = installTopSettingsPanelParking({ manager: service.manager });
                installExtensionsSubtreeGates({ manager: service.manager });
                validatePanelRuntimeInvariants({ profileName: service.manager.profile, pinnedSelectors });
            } catch (error) {
                console.error('TauriTavern: Panel Runtime install failed:', error);
                window.toastr?.error?.(t`Panel Runtime failed to install and is inactive. Check the console for details.`);
            }
        });

        return service;
    });

    return { ready };
}
