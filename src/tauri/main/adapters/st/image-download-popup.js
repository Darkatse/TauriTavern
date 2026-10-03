// @ts-check

import { translateSillyTavern } from './sillytavern-i18n.js';

/**
 * @param {{ src: string, alt: string }} source
 * @returns {Promise<boolean>}
 */
export async function confirmImageDownload({ src, alt }) {
    const { callGenericPopup, POPUP_RESULT, POPUP_TYPE } = await import(
        '../../../../scripts/popup.js'
    );

    const content = document.createElement('div');
    const title = document.createElement('h3');
    title.textContent = translateSillyTavern(
        'tauritavern_image_download_title',
        'Save this image?',
    );

    const preview = document.createElement('img');
    preview.src = src;
    preview.alt = alt;
    preview.style.maxWidth = '100%';
    preview.style.maxHeight = '50vh';
    preview.style.objectFit = 'contain';
    content.append(title, preview);

    const result = await callGenericPopup(content, POPUP_TYPE.CONFIRM, '', {
        okButton: translateSillyTavern('tauritavern_image_download_save', 'Save'),
        cancelButton: translateSillyTavern('tauritavern_image_download_cancel', 'Cancel'),
    });

    return result === POPUP_RESULT.AFFIRMATIVE;
}
