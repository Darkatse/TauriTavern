import { writeClipboardText } from '../../../tauri-bridge.js';
import { t } from '../../i18n.js';
import { extractErrorText } from '../../util/command-error-utils.js';
import { requestSafeMode } from './safe-mode.js';
import { openStartupDialog } from './startup-dialog.js';

const OVERLAY_ID = 'tt-startup-status-overlay';
const FAILURE_ID = 'tt-startup-failure';

function ensureOverlayElement() {
    const existing = document.getElementById(OVERLAY_ID);
    if (existing instanceof HTMLDivElement) {
        return existing;
    }

    const element = document.createElement('div');
    element.id = OVERLAY_ID;
    element.style.position = 'fixed';
    element.style.left = '12px';
    element.style.bottom = '12px';
    element.style.zIndex = '999999';
    element.style.maxWidth = 'min(92vw, 560px)';
    element.style.padding = '6px 10px';
    element.style.borderRadius = '10px';
    element.style.background = 'rgba(0, 0, 0, 0.55)';
    element.style.color = '#fff';
    element.style.font = '12px/1.4 system-ui, -apple-system, Segoe UI, Roboto, sans-serif';
    element.style.pointerEvents = 'none';
    element.style.whiteSpace = 'pre-wrap';
    element.textContent = '';

    document.body.appendChild(element);
    return element;
}

export function createStartupStatusOverlay() {
    const element = ensureOverlayElement();

    return {
        setText(text) {
            element.textContent = String(text ?? '');
        },
        remove() {
            element.remove();
        },
    };
}

/**
 * Callers must still throw the error; this only presents it.
 * @param {unknown} error
 */
export function showStartupFailure(error) {
    const stage = globalThis.__TAURITAVERN_STARTUP_STAGE__;
    const details = [extractErrorText(error), stage && `Stage: ${stage}`, /** @type {any} */ (error)?.stack]
        .filter(Boolean).join('\n');

    const dialog = openStartupDialog(FAILURE_ID);
    const content = {
        title: t`TauriTavern could not start`,
        description: [
            t`Try reloading first. If it still won't start, enter safe mode, or copy the details below to report the problem.`,
            t`If next time even this message doesn't appear, fully quit and reopen TauriTavern 5 times within 3 minutes to enter safe mode.`,
        ].join('\n\n'),
        details,
        actions: [
            {
                name: 'copy',
                label: t`Copy details`,
                run: async button => {
                    await writeClipboardText(details);
                    button.textContent = t`Copied!`;
                },
            },
            { name: 'safe-mode', label: t`Enter safe mode`, run: () => requestSafeMode(dialog, content) },
            { name: 'reload', label: t`Reload`, run: () => location.reload() },
        ],
    };
    dialog.show(content);
}
