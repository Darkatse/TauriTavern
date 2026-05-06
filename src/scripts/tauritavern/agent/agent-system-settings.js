export const AGENT_SYSTEM_MODULE_NAME = 'agent-system';
export const AGENT_SYSTEM_SETTINGS_KEY = 'settings';
export const AGENT_SYSTEM_SETTINGS_CHANGED = 'tauritavern-agent-system-settings-changed';
export const DEFAULT_AGENT_PROFILE_ID = 'default-writer';

export const DEFAULT_AGENT_SYSTEM_SETTINGS = Object.freeze({
    agentModeEnabled: false,
    selectedProfileId: DEFAULT_AGENT_PROFILE_ID,
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

function mergeSettings(value) {
    return {
        ...DEFAULT_AGENT_SYSTEM_SETTINGS,
        ...(value || {}),
    };
}

function emitSettingsChanged(settings) {
    window.dispatchEvent(new CustomEvent(AGENT_SYSTEM_SETTINGS_CHANGED, {
        detail: { settings },
    }));
}

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

export async function patchAgentSystemSettings(current, patch) {
    return saveAgentSystemSettings({
        ...mergeSettings(current),
        ...(patch || {}),
    });
}

export function subscribeAgentSystemSettings(listener) {
    const handler = (event) => listener(event.detail.settings);
    window.addEventListener(AGENT_SYSTEM_SETTINGS_CHANGED, handler);
    return () => window.removeEventListener(AGENT_SYSTEM_SETTINGS_CHANGED, handler);
}
