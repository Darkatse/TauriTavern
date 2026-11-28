import { callGenericPopup, POPUP_RESULT, POPUP_TYPE } from '../../popup.js';
import { translate } from '../../i18n.js';
import { writeClipboardText } from '../../../tauri-bridge.js';
import {
    isLocalStorageNearlyFull,
    listIndexedDbNames,
    readLocalStorageUsage,
    scheduleWebStorageCleanup,
} from '../../../tauri/main/services/web-storage/web-storage.js';
import { formatBytes } from './format-bytes.js';
import { callTauriTavernPanelPopup } from './panel-popup.js';
import { runOrPopup, runTaskOrPopup } from './setting-panel/popup-utils.js';

const WEB_STORAGE_STYLE_ID = 'tauritavern-web-storage-style';

function ensureWebStorageStyle() {
    if (document.getElementById(WEB_STORAGE_STYLE_ID)) {
        return;
    }

    const link = document.createElement('link');
    link.id = WEB_STORAGE_STYLE_ID;
    link.rel = 'stylesheet';
    link.href = new URL('./web-storage-app.css', import.meta.url).href;
    document.head.appendChild(link);
}

/** @param {{ localStorageKeys: string[], databases: string[] }} selection */
async function confirmCleanup(selection) {
    const count = selection.localStorageKeys.length + selection.databases.length;
    const content = document.createElement('div');
    content.className = 'flex-container flexFlowColumn';
    const title = document.createElement('b');
    title.textContent = translate('Delete {count} selected items?').replace('{count}', String(count));
    const explanation = document.createElement('div');
    explanation.textContent = translate(
        'This cannot be undone. Extensions or character cards that use this data may lose settings, records or caches. The app will then reload.',
    );
    content.append(title, explanation);

    const result = await callGenericPopup(content, POPUP_TYPE.CONFIRM, '', {
        okButton: translate('Delete and Reload'),
        cancelButton: translate('Cancel'),
        wide: false,
        large: false,
    });
    if (result === POPUP_RESULT.AFFIRMATIVE) {
        scheduleWebStorageCleanup(selection);
        window.location.reload();
    }
}

export async function openWebStorageManager() {
    ensureWebStorageStyle();
    const [bundle, databases] = await Promise.all([
        import(new URL('./dist/web-storage.bundle.js', import.meta.url).href),
        listIndexedDbNames(),
    ]);
    const usage = readLocalStorageUsage();

    const mount = document.createElement('div');
    const appHandle = bundle.mountTauriTavernWebStorageApp(mount, {
        snapshot: {
            localStorage: { ...usage, nearlyFull: isLocalStorageNearlyFull(usage) },
            databases,
        },
        actions: {
            cleanUp: confirmCleanup,
            copyText: text => runTaskOrPopup(async () => {
                await writeClipboardText(text);
                toastr.info(translate('Copied!'), '', { timeOut: 2000 });
            }),
        },
        tr: translate,
    });

    try {
        await callTauriTavernPanelPopup(mount, POPUP_TYPE.TEXT, '', {
            okButton: translate('Close'),
            allowVerticalScrolling: true,
            wide: false,
            large: false,
        });
    } finally {
        appHandle.unmount();
    }
}

/** @param {import('../../../tauri/main/services/web-storage/web-storage.js').WebStorageCleanupReport} report */
function showCleanupReport(report) {
    const parts = [];
    if (report.removedKeys > 0) {
        parts.push(translate('Freed {size} of local storage.').replace('{size}', formatBytes(report.removedBytes)));
    }
    if (report.deletedDatabases > 0) {
        parts.push(translate('Deleted {count} databases from IndexedDB.').replace('{count}', String(report.deletedDatabases)));
    }
    toastr.success(parts.join(' '), translate('Web storage cleaned up'));

    if (report.failedDatabases.length > 0) {
        toastr.error(
            report.failedDatabases.map(failure => `${failure.name}: ${failure.message}`).join('; '),
            translate('Some IndexedDB data could not be deleted'),
            { timeOut: 0, extendedTimeOut: 0, closeButton: true },
        );
    }
}

/**
 * Opens the manager from User Settings, reports the cleanup scheduled by the previous page, then
 * warns once if localStorage is nearly full.
 * @param {Promise<import('../../../tauri/main/services/web-storage/web-storage.js').WebStorageCleanupReport | null>} cleanup
 */
export async function installWebStorage(cleanup) {
    document.getElementById('web_storage_button').addEventListener('click', () => runOrPopup(openWebStorageManager));

    try {
        const report = await cleanup;
        if (report) {
            showCleanupReport(report);
        }
    } catch (error) {
        console.error('Web storage cleanup failed:', error);
        toastr.error(String(error?.message ?? error), translate('Could not clean up web storage'));
    }

    if (isLocalStorageNearlyFull(readLocalStorageUsage())) {
        toastr.warning(
            translate('Extensions may fail to save data. Click here to review and free up space.'),
            translate('Web storage is nearly used up'),
            { timeOut: 15000, onclick: () => runOrPopup(openWebStorageManager) },
        );
    }
}
