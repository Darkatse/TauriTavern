import { TAURITAVERN_SETTINGS_BUTTON_ID } from './setting-panel/constants.js';
import { installPairingListener } from './setting-panel/pairing-listener.js';
import { installSyncListeners } from './setting-panel/sync-listeners.js';
import { runOrPopup } from './setting-panel/popup-utils.js';

export function installTauriTavernSettingsPanel() {
    installPairingListener();
    installSyncListeners();

    if (document.readyState === 'loading') {
        document.addEventListener('DOMContentLoaded', bindSettingsButton, { once: true });
        return;
    }

    bindSettingsButton();
}

function bindSettingsButton() {
    const button = document.getElementById(TAURITAVERN_SETTINGS_BUTTON_ID);
    if (!button) {
        return;
    }

    button.addEventListener('click', () => {
        runOrPopup(async () => {
            const { openTauriTavernSettingsPopup } = await import('./setting-panel/settings-popup.js');
            await openTauriTavernSettingsPopup();
        });
    });
}
