// @ts-check

/** @type {boolean | null} */
let enabled = null;

/** @param {Record<string, any>} settings */
export function initializeChatVirtualization(settings) {
    if (!settings || typeof settings !== 'object') {
        throw new TypeError('Chat virtualization requires canonical TauriTavern settings');
    }

    const nextEnabled = settings.chat_virtualization_enabled;
    if (typeof nextEnabled !== 'boolean') {
        throw new TypeError('Chat virtualization setting must be a boolean');
    }
    if (enabled !== null && enabled !== nextEnabled) {
        throw new Error(`Chat virtualization changed during this page lifetime: ${enabled} -> ${nextEnabled}`);
    }

    enabled = nextEnabled;
    return enabled;
}

export function isChatVirtualizationEnabled() {
    if (enabled === null) {
        throw new Error('Chat virtualization is not initialized');
    }
    return enabled;
}
