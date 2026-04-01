import { callGenericPopup, POPUP_TYPE, Popup } from '../../../popup.js';
import { t, translate } from '../../../i18n.js';
import { TT_SYNC_SERVERS_CHANGED_EVENT } from './constants.js';
import { formatBytes } from './formatters.js';

let syncListenerInstalled = false;
let syncProgressPopup = null;
let syncProgressElements = null;

export function installSyncListeners() {
    if (syncListenerInstalled) {
        return;
    }
    syncListenerInstalled = true;

    const listen = window.__TAURI__.event.listen;

    void (async () => {
        await listen('lan_sync:progress', (event) => {
            const payload = event.payload;

            ensureSyncProgressPopup('LAN Sync progress');
            updateSyncProgressPopup(payload);
        });

        await listen('lan_sync:completed', async (event) => {
            const payload = event.payload;

            if (syncProgressPopup) {
                await syncProgressPopup.completeAffirmative();
            }
            syncProgressPopup = null;
            syncProgressElements = null;

            const files = payload.files_total;
            const bytes = payload.bytes_total;
            const deleted = payload.files_deleted;
            const message = [
                translate('LAN Sync completed.'),
                t`Files: ${files}`,
                typeof deleted === 'number' && deleted > 0 ? t`Deleted: ${deleted}` : null,
                t`Bytes: ${formatBytes(bytes)}`,
                '',
                translate('The app will now reload to refresh data.'),
            ].filter(Boolean).join('\n');
            await callGenericPopup(message, POPUP_TYPE.TEXT, '', {
                okButton: translate('OK'),
                allowVerticalScrolling: true,
                wide: false,
                large: false,
            });

            window.location.reload();
        });

        await listen('lan_sync:error', async (event) => {
            const payload = event.payload;

            if (syncProgressPopup) {
                await syncProgressPopup.completeAffirmative();
            }
            syncProgressPopup = null;
            syncProgressElements = null;

            const message = translate(payload.message);
            await callGenericPopup(String(message), POPUP_TYPE.TEXT, '', {
                okButton: translate('OK'),
                allowVerticalScrolling: true,
                wide: false,
                large: false,
            });
        });

        await listen('tt_sync:progress', (event) => {
            const payload = event.payload;

            ensureSyncProgressPopup('TT-Sync progress');
            updateSyncProgressPopup(payload);
        });

        await listen('tt_sync:completed', async (event) => {
            const payload = event.payload;

            if (syncProgressPopup) {
                await syncProgressPopup.completeAffirmative();
            }
            syncProgressPopup = null;
            syncProgressElements = null;

            window.dispatchEvent(new Event(TT_SYNC_SERVERS_CHANGED_EVENT));

            const files = payload.files_total;
            const bytes = payload.bytes_total;
            const deleted = payload.files_deleted;
            const direction = payload.direction === 'Push' ? translate('Push') : translate('Pull');

            const message = [
                t`TT-Sync ${direction} completed.`,
                t`Files: ${files}`,
                typeof deleted === 'number' && deleted > 0 ? t`Deleted: ${deleted}` : null,
                t`Bytes: ${formatBytes(bytes)}`,
                payload.direction === 'Pull' ? '' : null,
                payload.direction === 'Pull' ? translate('The app will now reload to refresh data.') : null,
            ].filter(Boolean).join('\n');

            await callGenericPopup(message, POPUP_TYPE.TEXT, '', {
                okButton: translate('OK'),
                allowVerticalScrolling: true,
                wide: false,
                large: false,
            });

            if (payload.direction === 'Pull') {
                window.location.reload();
            }
        });

        await listen('tt_sync:error', async (event) => {
            const payload = event.payload;

            if (syncProgressPopup) {
                await syncProgressPopup.completeAffirmative();
            }
            syncProgressPopup = null;
            syncProgressElements = null;

            const message = translate(payload.message);
            await callGenericPopup(String(message), POPUP_TYPE.TEXT, '', {
                okButton: translate('OK'),
                allowVerticalScrolling: true,
                wide: false,
                large: false,
            });
        });
    })();
}

function ensureSyncProgressPopup(titleText) {
    if (syncProgressPopup) {
        if (syncProgressElements?.title && titleText) {
            syncProgressElements.title.textContent = translate(titleText);
        }
        return syncProgressPopup;
    }

    const root = document.createElement('div');
    root.className = 'flex-container flexFlowColumn';
    root.style.gap = '10px';

    const title = document.createElement('b');
    title.textContent = translate(titleText || 'Sync progress');
    root.appendChild(title);

    const phase = document.createElement('div');
    root.appendChild(phase);

    const counts = document.createElement('div');
    root.appendChild(counts);

    const bytes = document.createElement('div');
    root.appendChild(bytes);

    const current = document.createElement('div');
    current.style.wordBreak = 'break-word';
    current.style.opacity = '0.9';
    root.appendChild(current);

    syncProgressElements = { title, phase, counts, bytes, current };
    updateSyncProgressPopup({
        phase: 'Starting',
        files_done: 0,
        files_total: 0,
        bytes_done: 0,
        bytes_total: 0,
        current_path: null,
    });

    const popup = new Popup(root, POPUP_TYPE.DISPLAY, '', {
        allowVerticalScrolling: true,
        wide: false,
        large: false,
    });

    syncProgressPopup = popup;
    void popup.show().finally(() => {
        if (syncProgressPopup === popup) {
            syncProgressPopup = null;
            syncProgressElements = null;
        }
    });

    return popup;
}

function updateSyncProgressPopup(payload) {
    if (!syncProgressElements) {
        return;
    }

    const direction = payload.direction || null;
    const phase = payload.phase;
    const filesDone = payload.files_done;
    const filesTotal = payload.files_total;
    const bytesDone = payload.bytes_done;
    const bytesTotal = payload.bytes_total;
    const currentPath = payload.current_path;

    syncProgressElements.phase.textContent = direction
        ? t`Phase: ${translate(direction)} / ${translate(phase)}`
        : t`Phase: ${translate(phase)}`;
    syncProgressElements.counts.textContent = t`Files: ${filesDone}/${filesTotal}`;
    syncProgressElements.bytes.textContent = t`Bytes: ${formatBytes(bytesDone)}/${formatBytes(bytesTotal)}`;
    syncProgressElements.current.textContent = currentPath ? t`Current: ${currentPath}` : '';
}

