// @ts-check

import { getSillyTavernLocale } from '../../adapters/st/sillytavern-i18n.js';
import { streamResponse } from '../../http-utils.js';

/**
 * @typedef {import('../../context/types.js').TauriMainContext} TauriMainContext
 * @typedef {{ path: string; name: string; kind: 'character' | 'lorebook' }} StagedUrlContent
 */

const CONTENT_TYPES = { png: 'image/png', json: 'application/json' };

/**
 * @param {TauriMainContext['safeInvoke']} safeInvoke
 * @param {string} url
 * @returns {Promise<StagedUrlContent>}
 */
export function stageUrlContent(safeInvoke, url) {
    return safeInvoke('stage_content_from_url', { url, locale: getSillyTavernLocale() });
}

/**
 * @param {StagedUrlContent} content
 * @returns {StagedUrlContent}
 */
export function expectCharacter(content) {
    if (content.kind !== 'character') {
        throw new Error(`Expected character content, received: ${content.kind}`);
    }
    return content;
}

/**
 * Returns the bytes whole, not streamed: a caller's `blob()` on a streamed body pages to disk in Chromium.
 * @param {Pick<TauriMainContext, 'safeInvoke' | 'adoptStagedFile' | 'createByteStream'>} host
 * @param {StagedUrlContent} content
 * @returns {Promise<{ bytes: ArrayBuffer; type: string }>}
 */
export async function takeUrlContent({ safeInvoke, adoptStagedFile, createByteStream }, { path }) {
    const staged = adoptStagedFile(path);
    try {
        const rid = await safeInvoke('open_staged_file', { path });
        const bytes = await streamResponse(createByteStream(rid)).arrayBuffer();
        const format = /** @type {'png' | 'json'} */ (path.slice(path.lastIndexOf('.') + 1));
        return { bytes, type: CONTENT_TYPES[format] };
    } finally {
        await staged.cleanup();
    }
}
