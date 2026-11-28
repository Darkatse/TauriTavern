// @ts-check

import { buildThumbnailUrl } from '../../tauri/main/context/asset-path-helpers.js';

/** @type {boolean | null} */
let originalAvatars = null;

/**
 * Setting changes reload the page, so this snapshot stays fixed until then.
 * @param {Record<string, any>} settings
 */
export function initializeListThumbnails(settings) {
    const enabled = settings.avatar_persona_original_images_enabled;
    if (typeof enabled !== 'boolean') {
        throw new TypeError('The original avatars setting must be a boolean');
    }
    originalAvatars = enabled;
}

/**
 * Reuse the chat thumbnail URL/cache while originals are off.
 * @param {'avatar'|'persona'} type
 * @param {string} file
 * @returns {string}
 */
export function getListThumbnailUrl(type, file) {
    if (originalAvatars === null) {
        throw new Error('List thumbnails were requested before the TauriTavern settings loaded');
    }
    return buildThumbnailUrl(type, file, { list: originalAvatars });
}
