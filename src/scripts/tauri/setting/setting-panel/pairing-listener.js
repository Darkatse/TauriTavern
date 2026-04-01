import { callGenericPopup, POPUP_RESULT, POPUP_TYPE } from '../../../popup.js';
import { translate } from '../../../i18n.js';
import { LAN_SYNC_DEVICES_CHANGED_EVENT } from './constants.js';

let pairingListenerInstalled = false;

export function installPairingListener() {
    if (pairingListenerInstalled) {
        return;
    }
    pairingListenerInstalled = true;

    const invoke = window.__TAURI__.core.invoke;
    const listen = window.__TAURI__.event.listen;

    void (async () => {
        await listen('lan_sync:pair_request', async (event) => {
            const payload = event.payload;
            const requestId = payload.request_id;
            const peerDeviceName = payload.peer_device_name;
            const peerDeviceId = payload.peer_device_id;
            const peerIp = payload.peer_ip;

            const content = document.createElement('div');
            content.className = 'flex-container flexFlowColumn';
            content.style.gap = '10px';

            const title = document.createElement('b');
            title.textContent = translate('LAN Sync pairing request');
            content.appendChild(title);

            const details = document.createElement('div');
            details.className = 'flex-container flexFlowColumn';
            details.style.gap = '6px';

            const deviceLine = document.createElement('div');
            deviceLine.textContent = `${translate('Device')}: ${peerDeviceName} (${peerDeviceId})`;
            details.appendChild(deviceLine);

            const ipLine = document.createElement('div');
            ipLine.textContent = `${translate('IP')}: ${peerIp}`;
            details.appendChild(ipLine);

            content.appendChild(details);

            const result = await callGenericPopup(content, POPUP_TYPE.CONFIRM, '', {
                okButton: translate('Allow'),
                cancelButton: translate('Deny'),
                allowVerticalScrolling: true,
                wide: false,
                large: false,
            });

            const accept = result === POPUP_RESULT.AFFIRMATIVE;
            await invoke('lan_sync_confirm_pairing', { requestId, accept });
            if (accept) {
                window.dispatchEvent(new Event(LAN_SYNC_DEVICES_CHANGED_EVENT));
            }
        });
    })();
}

