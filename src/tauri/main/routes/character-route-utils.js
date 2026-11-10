import { getSillyTavernLocale } from '../adapters/st/sillytavern-i18n.js';
import { streamResponse } from '../http-utils.js';

const CARD_CONTENT_TYPES = { png: 'image/png', json: 'application/json' };

/** @param {unknown} error */
export function badRequestBody(error) {
    const message = error instanceof Error ? error.message : String(error);
    return { error: message.replace(/^Bad request:\s*/i, '') };
}

/** @param {unknown} error */
export function isBadRequestError(error) {
    const message = error instanceof Error ? error.message : String(error || '');
    return /^Bad request:/i.test(message);
}

/** @param {unknown} error */
export function isNotFoundError(error) {
    const message = error instanceof Error ? error.message : String(error || '');
    return /(?:^|\b)(?:not found:|entity not found:)/i.test(message);
}

export async function resolveRouteCharacterId(context, options) {
    try {
        return { characterId: await context.resolveCharacterId(options) };
    } catch (error) {
        if (isBadRequestError(error)) {
            return { responseBody: badRequestBody(error) };
        }
        throw error;
    }
}

export async function resolveExistingRouteCharacterId(context, options) {
    try {
        return { characterId: await context.resolveExistingCharacterId(options) };
    } catch (error) {
        if (isBadRequestError(error)) {
            return { responseBody: badRequestBody(error) };
        }
        throw error;
    }
}

/**
 * Returns the card whole, not streamed: a caller's `blob()` on a streamed body pages to disk in Chromium.
 * @returns {Promise<{ bytes: ArrayBuffer; type: string; name: string }>}
 */
export async function stagedCardBytes(context, url) {
    const { path, name } = await context.safeInvoke('stage_character_card_from_url', {
        url,
        locale: getSillyTavernLocale(),
    });
    const staged = context.adoptStagedFile(path);
    try {
        const bytes = await streamResponse(await context.createReadableFileStream(path)).arrayBuffer();
        return { bytes, type: CARD_CONTENT_TYPES[path.slice(path.lastIndexOf('.') + 1)], name };
    } finally {
        await staged.cleanup();
    }
}
