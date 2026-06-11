import { callGenericPopup, POPUP_TYPE } from '../../popup.js';
import { translate } from '../../i18n.js';
import { trimFrontendLogEntriesInPlace } from '../../../tauri/main/services/dev-logging/frontend-log-retention.js';
import { openFullscreenTextViewer } from './text-viewer-popup.js';
import { runTaskOrPopup, showErrorPopup } from './setting-panel/popup-utils.js';

const DEV_LOGS_STYLE_ID = 'tauritavern-dev-logs-style';

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
            await navigator.clipboard.writeText(String(text ?? ''));
        }),
        openTextViewer: (options) => runTaskOrPopup(async () => {
            await openFullscreenTextViewer(options);
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
        await callGenericPopup(mount, POPUP_TYPE.TEXT, '', {
            okButton: translate('Close'),
            allowVerticalScrolling: true,
            wide: true,
            large: true,
        });
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
        kind: 'live',
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
        kind: 'live',
        title: 'Backend Logs',
        initialEntries,
        client: {
            subscribe: (handler) => devApi.backendLogs.subscribe(handler),
        },
    });
}

export async function openLlmApiLogsPanel() {
    const devApi = getDevApi();
    const keep = await devApi.llmApiLogs.getKeep();
    const indexEntries = await devApi.llmApiLogs.index({ limit: keep });
    const currentId = indexEntries.at(-1)?.id ?? 0;
    let initialPreview = null;

    if (currentId) {
        try {
            initialPreview = await devApi.llmApiLogs.getPreview(currentId);
        } catch (error) {
            initialPreview = {
                id: currentId,
                error: String(error),
            };
        }
    }

    await openDevLogsPopup({
        kind: 'llm-api',
        initialKeep: keep,
        initialIndexEntries: indexEntries,
        initialPreview,
        client: {
            index: (options) => devApi.llmApiLogs.index(options),
            getPreview: (id) => devApi.llmApiLogs.getPreview(id),
            getRaw: (id) => devApi.llmApiLogs.getRaw(id),
            subscribeIndex: (handler) => devApi.llmApiLogs.subscribeIndex(handler),
            setKeep: (value) => devApi.llmApiLogs.setKeep(value),
        },
    });
}
