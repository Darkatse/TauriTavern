import { eventSource, event_types } from '../../events.js';
import { TAURITAVERN_SETTINGS_BUTTON_ID } from './setting-panel/constants.js';
import { installPairingListener } from './setting-panel/pairing-listener.js';
import { installSyncListeners } from './setting-panel/sync-listeners.js';

export async function installTauriTavernSettingsPanel() {
    const appReady = new Promise(resolve => eventSource.once(event_types.APP_READY, resolve));
    void Promise.all([installPairingListener(appReady), installSyncListeners(appReady)])
        .catch(error => console.error('Failed to install sync and pairing listeners:', error));

    // Receive host events now; importing application UI must wait for its initialization.
    await appReady;
    const { runOrPopup } = await import('./setting-panel/popup-utils.js');
    void import('./extension-menu-shortcuts.js')
        .then(({ renderExtensionMenuShortcuts }) => renderExtensionMenuShortcuts())
        .catch(error => console.error('Failed to install TauriTavern quick access menu:', error));

    document.getElementById(TAURITAVERN_SETTINGS_BUTTON_ID)?.addEventListener('click', () => {
        runOrPopup(async () => {
            const { openTauriTavernSettingsPopup } = await import('./setting-panel/settings-popup.js');
            await openTauriTavernSettingsPopup();
        });
    });
}
