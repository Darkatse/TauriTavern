// @ts-check

function requireTauriSkillApi() {
    const host = window.__TAURITAVERN__;
    if (!host) {
        return null;
    }
    const skillApi = host.api?.skill;
    if (!skillApi || typeof skillApi.retargetScope !== 'function') {
        throw new Error('TauriTavern Skill API retargetScope is unavailable');
    }
    return skillApi;
}

/**
 * @param {unknown} value
 * @param {string} label
 */
function requireNonEmptyString(value, label) {
    const text = String(value || '').trim();
    if (!text) {
        throw new Error(`${label} is required`);
    }
    return text;
}

/**
 * @param {{ apiId: string; oldName: string; newName: string }} input
 */
export async function retargetPresetSkillsAfterRename({ apiId, oldName, newName }) {
    const skillApi = requireTauriSkillApi();
    if (!skillApi) {
        return;
    }

    const normalizedApiId = requireNonEmptyString(apiId, 'apiId');
    const normalizedOldName = requireNonEmptyString(oldName, 'oldName');
    const normalizedNewName = requireNonEmptyString(newName, 'newName');
    if (normalizedOldName === normalizedNewName) {
        return;
    }

    await skillApi.retargetScope({
        fromScope: {
            kind: 'preset',
            apiId: normalizedApiId,
            name: normalizedOldName,
        },
        toScope: {
            kind: 'preset',
            apiId: normalizedApiId,
            name: normalizedNewName,
        },
    });
}
