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
