// @ts-check

import {
    clearLegacyEmbeddedRuntimeProfileName,
    setEmbeddedRuntimeBootstrapProfileName,
} from '../../../../tauri/main/services/embedded-runtime/embedded-runtime-profile-state.js';
import { DYNAMIC_THEME_CHANGED_EVENT } from '../../../../tauri/main/services/dynamic-theme/constants.js';
import { setChatHistoryBootstrapModeName } from '../../../../tauri/main/services/chat-history/chat-history-mode-state.js';
import { syncNativeRegexBackendEnabledFromSettings } from '../../regex/native-regex-settings.js';

/**
 * @param {ReturnType<import('./settings-patch.js').buildTauriTavernSettingsUpdate>} update
 * @param {Record<string, any>} updatedSettings
 */
export function applyTauriTavernSettingsUpdateEffects(update, updatedSettings) {
    const { changes, next } = update;

    if (changes.nativeRegexBackendEnabled) {
        syncNativeRegexBackendEnabledFromSettings(updatedSettings);
    }

    if (changes.dynamicTheme) {
        window.dispatchEvent(new CustomEvent(DYNAMIC_THEME_CHANGED_EVENT, {
            detail: updatedSettings.dynamic_theme,
        }));
    }

    if (changes.panelRuntimeProfile) {
        // Keep in sync with:
        // - src/tauri/main/services/panel-runtime/preinstall.js
        // - src/tauri/main/services/panel-runtime/install.js
        //
        // Mirror the chosen profile so bootstrap can synchronously honor `off`
        // before Tauri settings are loaded.
        localStorage.setItem('tt:panelRuntimeProfile', next.panelRuntimeProfile);
    }

    if (changes.embeddedRuntimeProfile) {
        setEmbeddedRuntimeBootstrapProfileName(next.embeddedRuntimeProfile);
        clearLegacyEmbeddedRuntimeProfileName();
    }

    if (changes.chatHistoryMode) {
        setChatHistoryBootstrapModeName(next.chatHistoryMode);
    }

    if (
        changes.panelRuntimeProfile
        || changes.embeddedRuntimeProfile
        || changes.chatHistoryMode
        || changes.avatarPersonaOriginalImagesEnabled
    ) {
        window.location.reload();
    }
}
