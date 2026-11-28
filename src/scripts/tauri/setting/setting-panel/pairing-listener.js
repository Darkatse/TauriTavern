import { invoke, listen } from '../../../../tauri-bridge.js';
import { getCurrentLocale } from '../../../i18n.js';
import { LAN_SYNC_DEVICES_CHANGED_EVENT } from './constants.js';

export async function installPairingListener(appReady) {
    await Promise.all([
        listen('lan_sync:pairing_completed', () => {
            window.dispatchEvent(new Event(LAN_SYNC_DEVICES_CHANGED_EVENT));
        }),
        listen('lan_sync:pairing_review', async (event) => {
            await appReady;
            await invoke('lan_sync_review_pairing', {
                requestId: event.payload.request_id,
                locale: getCurrentLocale(),
            });
        }),
    ]);
}
