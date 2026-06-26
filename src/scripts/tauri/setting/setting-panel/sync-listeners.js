import { callGenericPopup, POPUP_TYPE, Popup } from '../../../popup.js';
import { t, translate } from '../../../i18n.js';
import {
    SYNC_AUTOMATION_CHANGED_EVENT,
    SYNC_AUTOMATION_STATUS_CHANGED_EVENT,
    TT_SYNC_SERVERS_CHANGED_EVENT,
} from './constants.js';
import { formatBytes, formatTimestamp } from './formatters.js';

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
            if (isAutoSyncPayload(event.payload)) {
                return;
            }
            updateSyncProgress('LAN Sync progress', event.payload);
        });

        await listen('lan_sync:completed', async (event) => {
            const payload = event.payload;
            if (isAutoSyncPayload(payload)) {
                window.dispatchEvent(new Event(SYNC_AUTOMATION_CHANGED_EVENT));
                return;
            }

            await showLanSyncCompleted(payload);
        });

        await listen('lan_sync:error', async (event) => {
            if (isAutoSyncPayload(event.payload)) {
                window.dispatchEvent(new Event(SYNC_AUTOMATION_CHANGED_EVENT));
                return;
            }
            await closeSyncProgressPopup();
            await showSyncError(event.payload);
        });

        await listen('tt_sync:progress', (event) => {
            if (isAutoSyncPayload(event.payload)) {
                return;
            }
            updateSyncProgress('TT-Sync progress', event.payload);
        });

        await listen('sync_auto:status', () => {
            window.dispatchEvent(new Event(SYNC_AUTOMATION_STATUS_CHANGED_EVENT));
        });

        await listen('sync_auto:toast', (event) => {
            showSyncAutomationToast(event.payload);
            window.dispatchEvent(new Event(SYNC_AUTOMATION_STATUS_CHANGED_EVENT));
        });
    })();
}

export async function showSyncReportResult(report) {
    const result = report?.result;
    if (!result || result.status === 'remote_request_accepted') {
        return;
    }

    if (result.status === 'failed') {
        if (report?.job?.endpoint?.type === 'remote_server') {
            window.dispatchEvent(new Event(TT_SYNC_SERVERS_CHANGED_EVENT));
        }
        await closeSyncProgressPopup();
        await showSyncError({ message: result.message || 'Sync failed.' });
        return;
    }

    if (result.status !== 'completed') {
        return;
    }

    const summary = result.summary;
    if (!summary) {
        return;
    }

    if (report?.job?.endpoint?.type === 'lan_peer') {
        await showLanSyncCompleted(summary);
        return;
    }

    if (report?.job?.endpoint?.type === 'remote_server') {
        await showTtSyncCompleted({
            direction: report?.job?.intent === 'replicate_local_to_remote' ? 'Push' : 'Pull',
            ...summary,
        });
    }
}

function isAutoSyncPayload(payload) {
    return payload?.origin === 'auto';
}

async function showLanSyncCompleted(payload) {
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
}

async function showTtSyncCompleted(payload) {
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
        payload.direction === 'Pull'
            ? translate('The app will now reload to refresh data.')
            : null,
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
}

function showSyncAutomationToast(payload) {
    const detail = String(payload?.detail || '').trim();
    const message = [
        formatSyncAutomationToastMessage(payload),
        detail || null,
    ].filter(Boolean).join('\n');
    const title = translate('Auto sync');
    const options = { timeOut: 7000, extendedTimeOut: 3000, preventDuplicates: true };

    if (payload?.level === 'warning') {
        toastr.warning(message, title, options);
        return;
    }

    toastr.info(message, title, options);
}

function formatSyncAutomationToastMessage(payload) {
    const nextRunAtMs = payload?.next_run_at_ms ?? payload?.nextRunAtMs ?? null;
    if (nextRunAtMs) {
        const nextRun = formatTimestamp(nextRunAtMs);
        if (payload?.message === 'Auto sync upload has started as scheduled.') {
            return t`Auto sync upload has started as scheduled. Next sync time: ${nextRun}`;
        }
        if (payload?.message === 'Auto sync upload has completed as scheduled.') {
            return t`Auto sync upload has completed as scheduled. Next sync time: ${nextRun}`;
        }
    }

    return translate(payload?.message || 'Auto sync updated.');
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
