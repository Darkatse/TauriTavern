// @ts-check

function requireTauriAgentProfilesApi() {
    const host = window.__TAURITAVERN__;
    if (!host) {
        return null;
    }
    const profilesApi = host.api?.agent?.profiles;
    if (!profilesApi || typeof profilesApi.retargetPresetRefs !== 'function') {
        throw new Error('TauriTavern Agent Profile API retargetPresetRefs is unavailable');
    }
    return profilesApi;
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
export async function retargetAgentProfilesAfterPresetRename({ apiId, oldName, newName }) {
    const profilesApi = requireTauriAgentProfilesApi();
    if (!profilesApi) {
        return null;
    }

    const normalizedApiId = requireNonEmptyString(apiId, 'apiId');
    const normalizedOldName = requireNonEmptyString(oldName, 'oldName');
    const normalizedNewName = requireNonEmptyString(newName, 'newName');
    if (normalizedOldName === normalizedNewName) {
        return null;
    }

    return profilesApi.retargetPresetRefs({
        from: {
            apiId: normalizedApiId,
            name: normalizedOldName,
        },
        to: {
            apiId: normalizedApiId,
            name: normalizedNewName,
        },
    });
}
