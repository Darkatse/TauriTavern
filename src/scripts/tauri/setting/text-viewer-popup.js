import { POPUP_TYPE, Popup } from '../../popup.js';

/**
 * @param {{ title: string, text: string, wrap?: 'soft'|'hard'|'off' }} options
 * @returns {Promise<string|number|boolean|null|undefined>}
 */
export async function openFullscreenTextViewer({ title, text, wrap = 'soft' }) {
    const root = document.createElement('div');
    root.className = 'tt-fullscreen-text-viewer';

    const header = document.createElement('div');
    header.className = 'tt-fullscreen-text-viewer__header';

    const titleEl = document.createElement('strong');
    titleEl.className = 'tt-fullscreen-text-viewer__title';
    titleEl.textContent = title;
    header.appendChild(titleEl);

    const textarea = document.createElement('textarea');
    textarea.className = 'tt-fullscreen-text-viewer__textarea text_pole textarea_compact monospace';
    textarea.readOnly = true;
    textarea.spellcheck = false;
    textarea.wrap = wrap;
    textarea.value = text ?? '';
    textarea.setAttribute('aria-label', title);
    textarea.setAttribute('autofocus', '');

    root.appendChild(header);
    root.appendChild(textarea);

    const popup = new Popup(root, POPUP_TYPE.DISPLAY, '', {
        allowVerticalScrolling: false,
    });
    popup.dlg.classList.add('tt-fullscreen-text-viewer-popup');

    return popup.show();
}
