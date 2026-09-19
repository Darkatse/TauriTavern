// @ts-check

/**
 * @typedef {Record<string, unknown> & {
 *   agentModeEnabled: boolean;
 *   chatInputToggleHidden: boolean;
 *   activeProfileId: string;
 *   editingProfileId: string;
 *   activeTab: string;
 *   runTimelineHeightPx: number | null;
 * }} AgentSystemSettings
 */

const AGENT_SYSTEM_MODULE_NAME = 'agent-system';
const AGENT_SYSTEM_SETTINGS_KEY = 'settings';
const AGENT_SYSTEM_SETTINGS_CHANGED = 'tauritavern-agent-system-settings-changed';
export const DEFAULT_AGENT_PROFILE_ID = 'default-writer';

const DEFAULT_AGENT_SYSTEM_SETTINGS = Object.freeze({
    agentModeEnabled: false,
    chatInputToggleHidden: false,
    activeProfileId: DEFAULT_AGENT_PROFILE_ID,
    editingProfileId: DEFAULT_AGENT_PROFILE_ID,
    activeTab: 'profiles',
    runTimelineHeightPx: null,
});

function requireExtensionStore() {
    const store = window.__TAURITAVERN__?.api?.extension?.store;
    if (!store) {
        throw new Error('TauriTavern extension store API is unavailable');
    }
    return store;
}

/**
 * @param {unknown} value
 * @returns {AgentSystemSettings}
 */
function mergeSettings(value) {
    const source = value && typeof value === 'object' && !Array.isArray(value)
        ? /** @type {Record<string, unknown>} */ (value)
        : {};
    const legacyProfileId = normalizeProfileIdSetting(source.selectedProfileId);
    const sourceActiveProfileId = normalizeProfileIdSetting(source.activeProfileId);
    const merged = /** @type {AgentSystemSettings} */ ({
        ...DEFAULT_AGENT_SYSTEM_SETTINGS,
        ...source,
    });
    merged.activeProfileId = sourceActiveProfileId
        || legacyProfileId
        || DEFAULT_AGENT_PROFILE_ID;
    merged.editingProfileId = normalizeProfileIdSetting(source.editingProfileId)
        || (sourceActiveProfileId ? merged.activeProfileId : legacyProfileId)
        || merged.activeProfileId;
    delete merged.selectedProfileId;
    return merged;
}

/** @param {unknown} value */
function normalizeProfileIdSetting(value) {
    const profileId = String(value || '').trim();
    return profileId || '';
}

/** @param {AgentSystemSettings} settings */
function emitSettingsChanged(settings) {
    window.dispatchEvent(new CustomEvent(AGENT_SYSTEM_SETTINGS_CHANGED, {
        detail: { settings },
    }));
}

/** @returns {Promise<AgentSystemSettings>} */
export async function loadAgentSystemSettings() {
    const store = requireExtensionStore();
    if (typeof store.tryGetJson !== 'function') {
        throw new Error('TauriTavern extension store tryGetJson API is unavailable');
    }

    const result = await store.tryGetJson({
        namespace: AGENT_SYSTEM_MODULE_NAME,
        key: AGENT_SYSTEM_SETTINGS_KEY,
    });

    if (typeof result?.found !== 'boolean') {
        throw new Error('TauriTavern extension store tryGetJson returned an invalid response');
    }

    if (!result.found) {
        return { ...DEFAULT_AGENT_SYSTEM_SETTINGS };
    }

    return mergeSettings(result.value);
}

/**
 * @param {AgentSystemSettings} settings
 * @returns {Promise<AgentSystemSettings>}
 */
export async function saveAgentSystemSettings(settings) {
    const next = mergeSettings(settings);
    await requireExtensionStore().setJson({
        namespace: AGENT_SYSTEM_MODULE_NAME,
        key: AGENT_SYSTEM_SETTINGS_KEY,
        value: next,
    });
    emitSettingsChanged(next);
    return next;
}

/**
 * @param {AgentSystemSettings} current
 * @param {Partial<AgentSystemSettings>} patch
 * @returns {Promise<AgentSystemSettings>}
 */
export async function patchAgentSystemSettings(current, patch) {
    return saveAgentSystemSettings({
        ...mergeSettings(current),
        ...(patch || {}),
    });
}

/** @param {(settings: AgentSystemSettings) => void} listener */
export function subscribeAgentSystemSettings(listener) {
    /** @param {Event} event */
    const handler = (event) => listener(
        /** @type {CustomEvent<{ settings: AgentSystemSettings }>} */ (event).detail.settings,
    );
    window.addEventListener(AGENT_SYSTEM_SETTINGS_CHANGED, handler);
    return () => window.removeEventListener(AGENT_SYSTEM_SETTINGS_CHANGED, handler);
}
