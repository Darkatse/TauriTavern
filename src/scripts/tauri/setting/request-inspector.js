import { POPUP_TYPE } from '../../popup.js';
import { translate } from '../../i18n.js';
import { toUserFacingErrorText } from '../../util/user-facing-error.js';
import { writeClipboardText } from '../../../tauri-bridge.js';
import { createTauriTavernPanelPopup } from './panel-popup.js';
import { runOrPopup, runTaskOrPopup, showErrorPopup } from './setting-panel/popup-utils.js';
import { openFullscreenTextViewer } from './text-viewer-popup.js';

const STYLE_ID = 'tauritavern-request-inspector-style';
const POPUP_CLASS = 'tt-request-inspector-popup';
const PREVIEW_BUTTON_SELECTOR = '.tt-request-preview-button';

function ensureStyle() {
    if (document.getElementById(STYLE_ID)) {
        return;
    }

    const link = document.createElement('link');
    link.id = STYLE_ID;
    link.rel = 'stylesheet';
    link.href = new URL('./request-inspector-app.css', import.meta.url).href;
    document.head.appendChild(link);
}

/**
 * Opens the request inspector on the next request or on the newest sent one.
 * @param {{ select: 'preview' | 'latest' }} options
 */
export async function openRequestInspector({ select }) {
    const dev = window.__TAURITAVERN__?.api?.dev;
    if (!dev) {
        throw new Error('TauriTavern host dev API is unavailable');
    }
    ensureStyle();

    const [bundle, keep] = await Promise.all([
        import(new URL('./dist/request-inspector.bundle.js', import.meta.url).href),
        dev.llmApiLogs.getKeep(),
    ]);
    const entries = await dev.llmApiLogs.index({ limit: keep });

    const mount = document.createElement('div');
    mount.className = 'tt-ri-mount';
    const appHandle = bundle.mountTauriTavernRequestInspectorApp(mount, {
        keep,
        entries,
        select,
        client: {
            index: (options) => dev.llmApiLogs.index(options),
            getLogView: (id) => dev.requestInspector.getLogView(id),
            getLogText: (id) => dev.llmApiLogs.getPreview(id),
            getRaw: (id) => dev.llmApiLogs.getRaw(id),
            subscribeIndex: (handler) => dev.llmApiLogs.subscribeIndex(handler),
            setKeep: (value) => dev.llmApiLogs.setKeep(value),
            preview: () => dev.requestInspector.preview(),
        },
        actions: {
            copyText: (text) => runTaskOrPopup(async () => {
                await writeClipboardText(text);
                toastr.info(translate('Copied!'), '', { timeOut: 2000 });
            }),
            openTextViewer: (options) => runTaskOrPopup(() => openFullscreenTextViewer(options)),
            reportError: (error) => showErrorPopup(error),
            errorText: toUserFacingErrorText,
        },
        tr: translate,
    });

    // The island scrolls its own body under a fixed bar.
    const popup = createTauriTavernPanelPopup(mount, POPUP_TYPE.TEXT, '', {
        okButton: translate('Close'),
        allowVerticalScrolling: false,
        wide: true,
        large: true,
    });
    popup.dlg.classList.add(POPUP_CLASS);
    try {
        await popup.show();
    } finally {
        appHandle.unmount();
    }
}

/** The Prompt Manager rebuilds its header, so the button is found when clicked. */
export function installRequestPreviewButton() {
    document.getElementById('completion_prompt_manager').addEventListener('click', (event) => {
        if (event.target instanceof Element && event.target.closest(PREVIEW_BUTTON_SELECTOR)) {
            runOrPopup(() => openRequestInspector({ select: 'preview' }));
        }
    });
}
