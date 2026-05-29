import { requireLlmConnectionsApi, requireSillyTavernContext } from './host-api.js';
import {
    buildLlmConnectionFromModelTarget,
    MODEL_TARGET_KIND,
} from './model-target-conversion.js';

export {
    buildLlmConnectionFromModelTarget,
    findModelTargetForBinding,
    modelBindingFromTarget,
    modelTargetConnectionRef,
} from './model-target-conversion.js';

export function listSavedModelTargets() {
    const targets = requireSillyTavernContext().extensionSettings?.connectionManager?.modelTargets;
    if (!Array.isArray(targets)) {
        return [];
    }

    return targets
        .filter((target) => target?.kind === MODEL_TARGET_KIND && target.mode === 'cc')
        .map((target) => structuredClone(target))
        .sort((a, b) => String(a.name || '').localeCompare(String(b.name || '')));
}

export async function saveModelTargetAsLlmConnection(target) {
    const connection = buildLlmConnectionFromModelTarget(target);
    await requireLlmConnectionsApi().save({ connection });
    return connection;
}
