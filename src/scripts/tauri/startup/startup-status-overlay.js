import { writeClipboardText } from '../../../tauri-bridge.js';
import { t } from '../../i18n.js';
import { extractErrorText } from '../../util/command-error-utils.js';

const OVERLAY_ID = 'tt-startup-status-overlay';
const FAILURE_ID = 'tt-startup-failure';

const FAILURE_STYLE = `
    dialog {
        box-sizing: border-box;
        width: min(92vw, 560px);
        padding: 20px;
        border: 1px solid GrayText;
        border-radius: 12px;
        color-scheme: light dark;
        background: Canvas;
        color: CanvasText;
        font: 14px/1.5 system-ui, -apple-system, "Segoe UI", Roboto, sans-serif;
    }
    dialog::backdrop { background: rgb(0 0 0 / 0.55); }
    h2 { margin: 0 0 8px; font-size: 18px; }
    p { margin: 0 0 12px; }
    pre {
        max-height: 40vh;
        margin: 0 0 16px;
        padding: 8px;
        overflow: auto;
        border-radius: 6px;
        background: rgb(127 127 127 / 0.15);
        font: 12px/1.4 ui-monospace, Menlo, Consolas, monospace;
        white-space: pre-wrap;
        overflow-wrap: anywhere;
        -webkit-user-select: text;
        user-select: text;
    }
    .actions { display: flex; justify-content: flex-end; gap: 8px; }
    button { font: inherit; padding: 6px 14px; }
`;

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

    const host = document.createElement('div');
    host.id = FAILURE_ID;
    // Theme styles, inherited ones included, must not reach the dialog; keep this reset inline.
    host.style.all = 'initial';
    const root = host.attachShadow({ mode: 'open' });
    root.innerHTML = `<style>${FAILURE_STYLE}</style>
        <dialog role="alertdialog" aria-labelledby="title" aria-describedby="description">
            <h2 id="title">${t`TauriTavern could not start`}</h2>
            <p id="description">${t`Reload to try again. If this keeps happening, copy the details below into a bug report.`}</p>
            <pre></pre>
            <div class="actions">
                <button type="button" name="copy">${t`Copy details`}</button>
                <button type="button" name="reload" autofocus>${t`Reload`}</button>
            </div>
        </dialog>`;
    root.querySelector('pre').textContent = details;

    const copy = root.querySelector('button[name="copy"]');
    copy.addEventListener('click', async () => {
        await writeClipboardText(details);
        copy.textContent = t`Copied!`;
    });
    root.querySelector('button[name="reload"]').addEventListener('click', () => location.reload());

    const dialog = root.querySelector('dialog');
    // The dialog must stay open; nothing usable remains behind it.
    dialog.addEventListener('close', () => dialog.showModal());
    document.body.append(host);
    dialog.showModal();
}
