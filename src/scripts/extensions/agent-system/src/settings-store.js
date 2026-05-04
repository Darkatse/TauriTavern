import { DEFAULT_SETTINGS, MODULE_NAME, SETTINGS_KEY } from './constants.js';
import { errorText, requireExtensionStore } from './host-api.js';

function isMissingStoreValue(error) {
    const message = errorText(error).toLowerCase();
    return message.includes('not found')
        || message.includes('no such file')
        || message.includes('enoent')
        || message.includes('os error 2');
}

export async function loadSettings() {
    const store = requireExtensionStore();
    try {
        const saved = await store.getJson({ namespace: MODULE_NAME, key: SETTINGS_KEY });
        return { ...DEFAULT_SETTINGS, ...(saved || {}) };
    } catch (error) {
        if (!isMissingStoreValue(error)) {
            throw error;
        }
        await store.setJson({ namespace: MODULE_NAME, key: SETTINGS_KEY, value: DEFAULT_SETTINGS });
        return { ...DEFAULT_SETTINGS };
    }
}

export async function saveSettings(settings) {
    await requireExtensionStore().setJson({
        namespace: MODULE_NAME,
        key: SETTINGS_KEY,
        value: settings,
    });
    return settings;
}

export async function patchSettings(current, patch) {
    return saveSettings({ ...current, ...patch });
}
