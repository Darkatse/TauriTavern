import { callGenericPopup, POPUP_TYPE, Popup } from '../../../popup.js';
import { t, translate } from '../../../i18n.js';
import { TT_SYNC_SERVERS_CHANGED_EVENT } from './constants.js';
import { formatBytes } from './formatters.js';

const SYNC_STYLE_ID = 'tauritavern-sync-style';

let syncListenerInstalled = false;
let syncProgressPopup = null;
let syncProgressApp = null;
let syncProgressOpening = null;
let syncProgressState = null;

function ensureSyncStyle() {
    if (document.getElementById(SYNC_STYLE_ID)) {
        return;
    }

    const link = document.createElement('link');
    link.id = SYNC_STYLE_ID;
    link.rel = 'stylesheet';
    link.href = new URL('./sync-app.css', import.meta.url).href;
    document.head.appendChild(link);
}

async function importSyncBundle() {
    return import(new URL('../dist/sync.bundle.js', import.meta.url).href);
}

function getListen() {
    const listen = window.__TAURI__?.event?.listen;
    if (typeof listen !== 'function') {
        throw new Error('Tauri event API is unavailable');
    }
    return listen;
}

export function installSyncListeners() {
    if (syncListenerInstalled) {
        return;
    }
    syncListenerInstalled = true;

    const listen = getListen();

    void (async () => {
        await listen('lan_sync:progress', (event) => {
            updateSyncProgress('LAN Sync progress', event.payload);
        });

        await listen('lan_sync:completed', async (event) => {
            const payload = event.payload;

            await closeSyncProgressPopup();

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
            await closeSyncProgressPopup();
            await showSyncError(event.payload);
        });

        await listen('tt_sync:progress', (event) => {
            updateSyncProgress('TT-Sync progress', event.payload);
        });

        await listen('tt_sync:completed', async (event) => {
            const payload = event.payload;

            await closeSyncProgressPopup();
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
            await closeSyncProgressPopup();
            await showSyncError(event.payload);
        });
    })();
}

function updateSyncProgress(title, payload) {
    syncProgressState = {
        title,
        payload,
    };

    if (syncProgressApp) {
        syncProgressApp.update(syncProgressState);
        return;
    }

    void ensureSyncProgressPopup();
}

async function ensureSyncProgressPopup() {
    if (syncProgressPopup) {
        return syncProgressPopup;
    }
    if (syncProgressOpening) {
        return syncProgressOpening;
    }

    syncProgressOpening = (async () => {
        ensureSyncStyle();
        const bundle = await importSyncBundle();
        const mount = document.createElement('div');
        const initialState = syncProgressState || {
            title: 'Sync progress',
            payload: {
                phase: 'Starting',
                files_done: 0,
                files_total: 0,
                bytes_done: 0,
                bytes_total: 0,
                current_path: null,
            },
        };

        syncProgressApp = bundle.mountTauriTavernSyncProgressApp(mount, {
            ...initialState,
            tr: translate,
        });

        const popup = new Popup(mount, POPUP_TYPE.DISPLAY, '', {
            allowVerticalScrolling: true,
            wide: false,
            large: false,
        });

        syncProgressPopup = popup;
        void popup.show().finally(() => {
            cleanupSyncProgressPopup(popup);
        });

        return popup;
    })();

    return syncProgressOpening;
}

async function closeSyncProgressPopup() {
    if (syncProgressOpening) {
        await syncProgressOpening.catch(() => null);
    }

    const popup = syncProgressPopup;
    if (!popup) {
        syncProgressState = null;
        return;
    }

    await popup.completeAffirmative();
    cleanupSyncProgressPopup(popup);
}

function cleanupSyncProgressPopup(popup) {
    if (syncProgressPopup !== popup) {
        return;
    }

    syncProgressApp?.unmount();
    syncProgressPopup = null;
    syncProgressApp = null;
    syncProgressOpening = null;
    syncProgressState = null;
}

async function showSyncError(payload) {
    const message = translate(payload.message);
    await callGenericPopup(String(message), POPUP_TYPE.TEXT, '', {
        okButton: translate('OK'),
        allowVerticalScrolling: true,
        wide: false,
        large: false,
    });
}
