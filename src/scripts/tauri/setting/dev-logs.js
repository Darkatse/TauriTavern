import { POPUP_TYPE } from '../../popup.js';
import { translate } from '../../i18n.js';
import { trimFrontendLogEntriesInPlace } from '../../../tauri/main/services/dev-logging/frontend-log-retention.js';
import { writeClipboardText } from '../../../tauri-bridge.js';
import { runTaskOrPopup, showErrorPopup } from './setting-panel/popup-utils.js';
import { createTauriTavernPanelPopup } from './panel-popup.js';

const DEV_LOGS_STYLE_ID = 'tauritavern-dev-logs-style';
const DEV_LOGS_POPUP_CLASS = 'tt-dev-logs-popup';

function getDevApi() {
    const api = window.__TAURITAVERN__?.api?.dev;
    if (!api) {
        throw new Error('TauriTavern host dev API is unavailable');
    }
    return api;
}

function ensureDevLogsStyle() {
    if (document.getElementById(DEV_LOGS_STYLE_ID)) {
        return;
    }

    const link = document.createElement('link');
    link.id = DEV_LOGS_STYLE_ID;
    link.rel = 'stylesheet';
    link.href = new URL('./dev-logs-app.css', import.meta.url).href;
    document.head.appendChild(link);
}

async function importDevLogsBundle() {
    return import(new URL('./dist/dev-logs.bundle.js', import.meta.url).href);
}

function createActions() {
    return {
        copyText: (text) => runTaskOrPopup(async () => {
            await writeClipboardText(String(text ?? ''));
        }),
        reportError: (error) => showErrorPopup(error),
    };
}

async function openDevLogsPopup(options) {
    ensureDevLogsStyle();

    const bundle = await importDevLogsBundle();
    const mount = document.createElement('div');
    const appHandle = bundle.mountTauriTavernDevLogsApp(mount, {
        ...options,
        actions: createActions(),
        tr: translate,
    });

    try {
        const popup = createTauriTavernPanelPopup(mount, POPUP_TYPE.TEXT, '', {
            okButton: translate('Close'),
            allowVerticalScrolling: true,
        });
        popup.dlg.classList.add(DEV_LOGS_POPUP_CLASS);
        await popup.show();
    } finally {
        appHandle.unmount();
    }
}

export async function openFrontendLogsPanel() {
    const devApi = getDevApi();
    const [consoleCaptureEnabled, initialEntries] = await Promise.all([
        devApi.frontendLogs.getConsoleCaptureEnabled(),
        devApi.frontendLogs.list(),
    ]);

    await openDevLogsPopup({
        title: 'Frontend Logs',
        initialEntries,
        consoleCaptureEnabled,
        showConsoleCapture: true,
        trimEntriesInPlace: trimFrontendLogEntriesInPlace,
        client: {
            subscribe: (handler) => devApi.frontendLogs.subscribe(handler),
            setConsoleCaptureEnabled: (enabled) => devApi.frontendLogs.setConsoleCaptureEnabled(enabled),
        },
    });
}

export async function openBackendLogsPanel() {
    const devApi = getDevApi();
    const initialEntries = await devApi.backendLogs.tail({ limit: 800 });

    await openDevLogsPopup({
        title: 'Backend Logs',
        initialEntries,
        client: {
            subscribe: (handler) => devApi.backendLogs.subscribe(handler),
        },
    });
}
