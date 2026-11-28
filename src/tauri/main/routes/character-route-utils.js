import { commandErrorStatus, getCommandErrorMessage } from '../../../scripts/util/command-error-utils.js';

export async function resolveRouteCharacterId(context, options) {
    try {
        return { characterId: await context.resolveCharacterId(options) };
    } catch (error) {
        if (commandErrorStatus(error) === 400) {
            return { responseBody: { error: getCommandErrorMessage(error) } };
        }
        throw error;
    }
}

export async function resolveExistingRouteCharacterId(context, options) {
    try {
        return { characterId: await context.resolveExistingCharacterId(options) };
    } catch (error) {
        if (commandErrorStatus(error) === 400) {
            return { responseBody: { error: getCommandErrorMessage(error) } };
        }
        throw error;
    }
}

